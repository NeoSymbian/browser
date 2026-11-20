// SPDX-License-Identifier: Apache-2.0
//! `neowebe --serve` (browser WB4c): the engine process behind NEOWEB. An IPC thread runs the server
//! [`neoweb_proto::SERVER_NAME`]; the main thread runs Servo, takes the commands it queues and posts events and
//! frames back (protocol: app/neoweb-proto).
//!
//! - **Threads:** Servo's WebView is `Rc` and stays on the main thread. The IPC thread owns the e32 `Server` (bound
//!   to the thread that created it); both share [`Shared`] under one mutex. A held NEXT_EVENT [`Message`] is
//!   completed by whichever thread has the next event: message handles are valid in every thread of the process.
//! - **Frames:** one global chunk, two RGBA8 buffers sized for [`MAX_W`] × [`MAX_H`]. A buffer is written only while
//!   NEOWEB doesn't hold it (between [`Event::Frame`] and [`Command::FrameShown`]), so painting never outruns showing.
//! - **Lifetime:** the server is transient: when the last session closes, the engine exits; NEOWEB starts it again.

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dpi::PhysicalSize;
use e32::chunk::Chunk;
use e32::server::{Message, Server, Service};
use e32::sys::TVersion;
use euclid::Scale;
use neoweb_proto::{Command, Event, FrameHeader, Load, Rect, TouchKind, func};
use servo::{
    DevicePoint, EventLoopWaker, InputEvent, LoadStatus, TouchEvent, TouchEventType, TouchId,
    TouchPointerType, WebView, WebViewBuilder, WebViewDelegate, WebViewPoint, WheelDelta,
    WheelEvent, WheelMode,
};

use crate::Waker;

/// The largest page area a frame buffer holds (the Pixel 3a XL's 1080 px width, its full height).
pub const MAX_W: u32 = 1080;
pub const MAX_H: u32 = 2400;
const KERR_CANCEL: i32 = -3;
const KERR_NOT_SUPPORTED: i32 = -5;
const KERR_ARGUMENT: i32 = -6;
const KERR_IN_USE: i32 = -14;

/// What the IPC thread and the main thread share.
#[derive(Default)]
struct Shared {
    cmds: VecDeque<Command>,
    events: VecDeque<Event>,
    /// The session's outstanding NEXT_EVENT.
    held: Option<Message>,
    /// Sessions open; the engine exits when the last one closes.
    sessions: u32,
    /// A new client: it holds no buffer.
    reset: bool,
    quit: bool,
}

impl Shared {
    /// Deliver `e` to the held NEXT_EVENT, or queue it.
    fn post(&mut self, e: Event) {
        match self.held.take() {
            Some(m) => complete_event(m, &e),
            None => self.events.push_back(e),
        }
    }
}

fn complete_event(m: Message, e: &Event) {
    let b = e.encode();
    match m.write8(0, &b, 0) {
        Ok(()) => m.complete(b.len() as i32),
        Err(err) => m.complete(err),
    }
}

type SharedRef = Arc<Mutex<Shared>>;

/// The server side: runs on the IPC thread.
struct Engine {
    shared: SharedRef,
    waker: Waker,
    chunk_handle: i32,
}

impl Service for Engine {
    type Session = ();

    fn connect(&mut self, v: TVersion) -> Result<(), i32> {
        let (major, _, _) = neoweb_proto::VERSION;
        if v.major as u8 != major {
            return Err(KERR_NOT_SUPPORTED);
        }
        let mut s = self.shared.lock().unwrap();
        s.sessions += 1;
        s.reset = true;
        Ok(())
    }

    fn message(&mut self, _: &mut (), m: Message) {
        match m.function() {
            func::COMMAND => {
                let cmd = m.read8_vec(0).ok().and_then(|b| Command::decode(&b).ok());
                match cmd {
                    Some(c) => {
                        self.shared.lock().unwrap().cmds.push_back(c);
                        m.complete(0);
                        self.waker.wake();
                    }
                    None => m.complete(KERR_ARGUMENT),
                }
            }
            func::NEXT_EVENT => {
                let mut s = self.shared.lock().unwrap();
                match s.events.pop_front() {
                    Some(e) => complete_event(m, &e),
                    // shortcut: one held request for the whole engine (one NEOWEB); per-session queues if a second
                    // client ever connects
                    None if s.held.is_some() => m.complete(KERR_IN_USE),
                    None => s.held = Some(m),
                }
            }
            func::FRAMES => m.complete_with_handle(self.chunk_handle),
            func::CANCEL => {
                if let Some(h) = self.shared.lock().unwrap().held.take() {
                    h.complete(KERR_CANCEL);
                }
                m.complete(0);
            }
            _ => m.complete(KERR_NOT_SUPPORTED),
        }
    }

    fn disconnect(&mut self, _: ()) -> std::ops::ControlFlow<()> {
        let mut s = self.shared.lock().unwrap();
        if let Some(h) = s.held.take() {
            h.complete(KERR_CANCEL);
        }
        s.events.clear(); // a client that comes next starts afresh (and gets the frame buffers back: `reset`)
        s.sessions -= 1;
        if s.sessions == 0 {
            s.quit = true;
            drop(s);
            self.waker.wake();
            return std::ops::ControlFlow::Break(());
        }
        std::ops::ControlFlow::Continue(())
    }
}

/// Servo's notifications, turned into protocol events.
struct Delegate {
    shared: SharedRef,
    frame_ready: std::cell::Cell<bool>,
}

impl Delegate {
    fn post(&self, e: Event) {
        self.shared.lock().unwrap().post(e);
    }
}

impl WebViewDelegate for Delegate {
    fn notify_load_status_changed(&self, _: WebView, status: LoadStatus) {
        self.post(Event::Load(match status {
            LoadStatus::Started => Load::Started,
            LoadStatus::HeadParsed => Load::HeadParsed,
            LoadStatus::Complete | LoadStatus::Stopped => Load::Complete,
        }));
    }

    fn notify_url_changed(&self, _: WebView, url: url::Url) {
        self.post(Event::Url(url.into()));
    }

    fn notify_page_title_changed(&self, _: WebView, title: Option<String>) {
        self.post(Event::Title(title.unwrap_or_default()));
    }

    fn notify_history_changed(&self, _: WebView, entries: Vec<url::Url>, current: usize) {
        self.post(Event::History {
            back: current > 0,
            forward: current + 1 < entries.len(),
        });
    }

    fn notify_new_frame_ready(&self, _: WebView) {
        self.frame_ready.set(true);
    }

    fn notify_crashed(&self, _: WebView, reason: String, _backtrace: Option<String>) {
        println!("neowebe: crashed: {reason}");
        self.post(Event::Crashed(reason));
    }
}

/// The two frame buffers: which frame (if any) NEOWEB still holds in each.
#[derive(Default)]
struct Buffers {
    held: [Option<u32>; 2],
    seq: u32,
}

impl Buffers {
    fn free(&self) -> Option<usize> {
        self.held.iter().position(Option::is_none)
    }

    fn shown(&mut self, seq: u32) {
        for h in &mut self.held {
            if *h == Some(seq) {
                *h = None;
            }
        }
    }
}

fn input(webview: &WebView, cmd: Command) {
    match cmd {
        Command::Touch { kind, id, x, y } => {
            let kind = match kind {
                TouchKind::Down => TouchEventType::Down,
                TouchKind::Move => TouchEventType::Move,
                TouchKind::Up => TouchEventType::Up,
                TouchKind::Cancel => TouchEventType::Cancel,
            };
            let point = WebViewPoint::Device(DevicePoint::new(x as f32, y as f32));
            let event = TouchEvent::new(kind, TouchId(id as i32), point, TouchPointerType::Touch);
            webview.notify_input_event(InputEvent::Touch(event));
        }
        Command::Scroll { dx, dy, x, y } => {
            // WheelDelta's y > 0 scrolls up: a positive dy (reveal more below) is a negative wheel y
            let delta = WheelDelta {
                x: -(dx as f64),
                y: -(dy as f64),
                z: 0.0,
                mode: WheelMode::DeltaPixel,
            };
            let point = WebViewPoint::Device(DevicePoint::new(x as f32, y as f32));
            webview.notify_input_event(InputEvent::Wheel(WheelEvent::new(delta, point)));
        }
        // shortcut: keys and text arrive in WB5 (focus, IME); dropped until then
        Command::Key { .. } | Command::Text(_) => {}
        _ => {}
    }
}

/// Run the engine server until the last client goes. Returns the exit reason.
pub fn serve(log: log::LevelFilter) -> Result<(), String> {
    let _ = log::set_logger(&crate::DebugLog);
    log::set_max_level(log);
    let (chunk_size, header) = FrameHeader::layout(MAX_W, MAX_H);
    let chunk =
        Chunk::create_global(chunk_size).map_err(|e| format!("frame chunk {chunk_size} B: {e}"))?;
    // SAFETY: the chunk is ours, committed and at least FrameHeader::SIZE long; NEOWEB only reads it
    unsafe { (chunk.base() as *mut FrameHeader).write(header) };

    let shared: SharedRef = Arc::default();
    let waker = Waker::default();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    {
        let (shared, waker, chunk_handle) = (shared.clone(), waker.clone(), chunk.handle());
        std::thread::Builder::new()
            .name("neoweb-ipc".into())
            .stack_size(64 * 1024)
            .spawn(move || {
                let server = match Server::create(neoweb_proto::SERVER_NAME) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                e32::process::rendezvous(0); // NEOWEB, waiting to connect, may go on
                let mut engine = Engine {
                    shared: shared.clone(),
                    waker: waker.clone(),
                    chunk_handle,
                };
                let r = e32::rt::block_on(e32::server::serve(&server, &mut engine));
                println!("neowebe: server ended: {r:?}");
                shared.lock().unwrap().quit = true;
                waker.wake();
            })
            .map_err(|e| format!("IPC thread: {e}"))?;
    }
    ready_rx
        .recv()
        .map_err(|_| "IPC thread died")?
        .map_err(|e| format!("server {}: {e}", neoweb_proto::SERVER_NAME))?;

    let mut size = PhysicalSize::new(MAX_W, 2037);
    let (servo, ctx) = crate::start_servo(size, &waker);
    let delegate = Rc::new(Delegate {
        shared: shared.clone(),
        frame_ready: Default::default(),
    });
    let webview = WebViewBuilder::new(&servo, ctx.clone())
        .hidpi_scale_factor(Scale::new(2.75))
        .delegate(delegate.clone())
        .build();
    let mut buffers = Buffers::default();
    let mut pending = false;
    println!("neowebe: serving");
    loop {
        let cmds: Vec<Command> = {
            let mut s = shared.lock().unwrap();
            if s.quit {
                break;
            }
            if std::mem::take(&mut s.reset) {
                buffers.held = [None, None];
                pending = true; // the new client sees the current page at once
            }
            s.cmds.drain(..).collect()
        };
        for cmd in cmds {
            match cmd {
                Command::Load(u) => match url::Url::parse(&u) {
                    Ok(u) => webview.load(u),
                    Err(e) => delegate.post(Event::Crashed(format!("{u}: {e}"))),
                },
                Command::Reload => webview.reload(),
                Command::Stop => webview.evaluate_javascript("window.stop()", |_| {}),
                Command::Back => {
                    webview.go_back(1);
                }
                Command::Forward => {
                    webview.go_forward(1);
                }
                Command::Resize { w, h, dpr_milli } => {
                    let new = PhysicalSize::new(w.clamp(1, MAX_W), h.clamp(1, MAX_H));
                    if new != size {
                        size = new;
                        webview.resize(size);
                    }
                    if dpr_milli > 0 {
                        webview.set_hidpi_scale_factor(Scale::new(dpr_milli as f32 / 1000.0));
                    }
                    pending = true;
                }
                Command::FrameShown { seq } => buffers.shown(seq),
                other => input(&webview, other),
            }
        }
        servo.spin_event_loop();
        if delegate.frame_ready.replace(false) {
            pending = true;
        }
        if pending && let Some(i) = buffers.free() {
            pending = false;
            webview.paint();
            if let Some(img) =
                ctx.read_to_image(servo::DeviceIntRect::from_size(ctx.size2d().to_i32()))
            {
                let (w, h) = img.dimensions();
                // shortcut: read_to_image allocates and copies a whole frame; write into the chunk straight from
                // SWGL's colour buffer if scrolling is too slow
                let dst = unsafe { chunk.base().add(header.offsets[i] as usize) };
                for (row, src) in img
                    .as_raw()
                    .chunks_exact(w as usize * 4)
                    .enumerate()
                    .take(MAX_H as usize)
                {
                    let n = src.len().min(header.stride as usize);
                    // SAFETY: row < MAX_H and n <= stride: inside buffer i of the chunk
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            src.as_ptr(),
                            dst.add(row * header.stride as usize),
                            n,
                        )
                    };
                }
                buffers.seq = buffers.seq.wrapping_add(1);
                buffers.held[i] = Some(buffers.seq);
                delegate.post(Event::Frame {
                    seq: buffers.seq,
                    buffer: i as u8,
                    width: w.min(MAX_W),
                    height: h.min(MAX_H),
                    dirty: Rect {
                        x: 0,
                        y: 0,
                        w: w as i32,
                        h: h as i32,
                    },
                });
            }
        }
        waker.wait(Duration::from_millis(20));
    }
    println!("neowebe: last client gone, exiting");
    Ok(())
}
