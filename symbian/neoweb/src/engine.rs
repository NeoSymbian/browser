// SPDX-License-Identifier: GPL-3.0-only
//! The session and frame buffers shared with NEOWEBE.EXE.

use alloc::{rc::Rc, vec, vec::Vec};
use core::pin::pin;
use e32::chunk::Chunk;
use e32::ipc::{Session, TIpcArgs, TVersion};
use e32::process::Process;
use e32::sys::{self, TPtr8, TPtrC8, err};
use neoweb_proto::{Command, EVENT_MAX, Event, FrameHeader, SERVER_NAME, VERSION, func};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

const KERR_CORRUPT: i32 = -20;

fn connect() -> Result<Session, i32> {
    let (major, minor, build) = VERSION;
    Session::connect(
        SERVER_NAME,
        TVersion {
            major,
            minor,
            build,
        },
    )
}

async fn start_server() -> Result<Session, i32> {
    match connect() {
        Err(err::KERR_NOT_FOUND | err::KERR_SERVER_TERMINATED) => {}
        result => return result,
    }
    let process = Process::spawn("neowebe.exe", "--serve")?;
    process.resume();
    for _ in 0..100 {
        e32::time::delay(100_000).await;
        if let Some((_, reason)) = process.exit_info() {
            return Err(if reason < 0 {
                reason
            } else {
                err::KERR_SERVER_TERMINATED
            });
        }
        match connect() {
            Err(err::KERR_NOT_FOUND | err::KERR_SERVER_TERMINATED) => {}
            result => return result,
        }
    }
    Err(err::KERR_TIMED_OUT)
}

pub struct Engine {
    session: Rc<Session>,
    chunk: Chunk,
    header: FrameHeader,
}

impl Engine {
    pub async fn start() -> Result<Self, i32> {
        let session = start_server().await?;
        let handle = session.send_receive(func::FRAMES, &TIpcArgs::new())?;
        if handle == 0 {
            return Err(KERR_CORRUPT);
        }
        let chunk = Chunk::from_handle(handle);
        // SAFETY: Exec::ChunkSize takes the open chunk handle in r0 and returns its committed byte count.
        let size =
            unsafe { sys::swi::exec4::<{ sys::execs::slow::CHUNK_SIZE }>(handle as u32, 0, 0, 0) }
                as i32;
        if chunk.base().is_null() || size < FrameHeader::SIZE as i32 {
            return Err(KERR_CORRUPT);
        }
        // SAFETY: FRAMES publishes the initialized repr(C) header at the page-aligned start of this mapping.
        let header = unsafe { chunk.base().cast::<FrameHeader>().read_volatile() };
        let [first, second] = header.offsets;
        let bytes = header
            .stride
            .checked_mul(header.height)
            .ok_or(KERR_CORRUPT)?;
        if header.magic != FrameHeader::MAGIC
            || header.width == 0
            || header.height == 0
            || header.width > header.stride / 4
            || first < FrameHeader::SIZE as u32
            || second.checked_sub(first).is_none_or(|room| room < bytes)
            || (size as u32)
                .checked_sub(second)
                .is_none_or(|room| room < bytes)
        {
            return Err(KERR_CORRUPT);
        }
        Ok(Self {
            session: Rc::new(session),
            chunk,
            header,
        })
    }

    /// The caller keeps this watch across UI events and timers, until the engine disconnects.
    pub fn watch(&self) -> Watch {
        Watch {
            session: self.session.clone(),
            bytes: vec![0; EVENT_MAX],
        }
    }

    pub fn send(&self, command: Command) -> Result<(), i32> {
        let bytes = command.encode();
        let descriptor = TPtrC8::new(&bytes);
        let mut args = TIpcArgs::new();
        args.desc8(0, &descriptor);
        self.session.send_receive(func::COMMAND, &args).map(|_| ())
    }

    /// Copy a published frame while its buffer is held. The caller returns it with FrameShown after installing the image.
    pub fn frame(&self, buffer: u8, width: u32, height: u32) -> Result<Image, i32> {
        let &offset = self
            .header
            .offsets
            .get(usize::from(buffer))
            .ok_or(KERR_CORRUPT)?;
        if width == 0 || height == 0 || width > self.header.width || height > self.header.height {
            return Err(KERR_CORRUPT);
        }
        let stride = self.header.stride as usize;
        let row_bytes = width as usize * 4;
        // SAFETY: start checked both buffer extents against the committed mapping. Only this published buffer is
        // borrowed: the engine cannot write to it again until the caller sends FrameShown.
        let source = unsafe {
            core::slice::from_raw_parts(
                self.chunk.base().add(offset as usize),
                stride * height as usize,
            )
        };
        // shortcut: one copy per frame plus Slint's blit, use a neoapp raw blit if measured slow.
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        for (dst, src) in pixels
            .make_mut_bytes()
            .chunks_exact_mut(row_bytes)
            .zip(source.chunks_exact(stride))
        {
            dst.copy_from_slice(&src[..row_bytes]);
        }
        Ok(Image::from_rgba8(pixels))
    }
}

pub struct Watch {
    session: Rc<Session>,
    bytes: Vec<u8>,
}

// Dropping a pending e32 SendFuture waits for completion. Cancel on its session before dropping the future/descriptor.
struct Cancel<'a>(&'a Session, bool);

impl Drop for Cancel<'_> {
    fn drop(&mut self) {
        if !self.1 {
            let _ = self.0.send_receive(func::CANCEL, &TIpcArgs::new());
        }
    }
}

impl Watch {
    pub async fn next(mut self) -> (Self, Result<Event, i32>) {
        let mut descriptor = TPtr8::new(&mut self.bytes);
        let result = {
            let mut args = TIpcArgs::new();
            args.des8(0, &mut descriptor);
            let future = pin!(self.session.send(func::NEXT_EVENT, &args));
            let mut cancel = Cancel(&self.session, false);
            let result = future.await;
            cancel.1 = true;
            result
        };
        let event = result.and_then(|n| {
            if n as usize != descriptor.len() {
                return Err(KERR_CORRUPT);
            }
            let bytes = self.bytes.get(..n as usize).ok_or(KERR_CORRUPT)?;
            Event::decode(bytes).map_err(|_| KERR_CORRUPT)
        });
        (self, event)
    }
}

type Request<T> = core::pin::Pin<alloc::boxed::Box<dyn core::future::Future<Output = T>>>;

/// Own the pinned operation between calls to select, including while a fresh engine is starting.
pub enum Connection {
    Starting(Request<Result<Engine, i32>>),
    Online {
        engine: Engine,
        next: Request<(Watch, Result<Event, i32>)>,
    },
    Offline,
}

pub enum Update {
    Connected,
    Event(Event),
}

impl Connection {
    pub fn start() -> Self {
        Self::Starting(alloc::boxed::Box::pin(Engine::start()))
    }

    pub async fn next(&mut self) -> Result<Update, i32> {
        match self {
            Self::Starting(start) => {
                let engine = start.await?;
                let next = alloc::boxed::Box::pin(engine.watch().next());
                *self = Self::Online { engine, next };
                Ok(Update::Connected)
            }
            Self::Online { next, .. } => {
                let (watch, event) = next.as_mut().await;
                *next = alloc::boxed::Box::pin(watch.next());
                event.map(Update::Event)
            }
            Self::Offline => core::future::pending().await,
        }
    }

    pub fn send(&mut self, command: Command) -> Result<(), i32> {
        match self {
            Self::Online { engine, .. } => engine.send(command),
            Self::Offline if matches!(command, Command::Load(_)) => {
                *self = Self::start();
                Ok(())
            }
            // Connected sends the latest viewport and requested URL, including edits made during launch.
            Self::Offline | Self::Starting(_) => Ok(()),
        }
    }

    pub fn frame(&self, buffer: u8, width: u32, height: u32) -> Result<Image, i32> {
        match self {
            Self::Online { engine, .. } => engine.frame(buffer, width, height),
            _ => Err(err::KERR_SERVER_TERMINATED),
        }
    }
}
