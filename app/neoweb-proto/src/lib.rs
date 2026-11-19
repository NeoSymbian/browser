// SPDX-License-Identifier: Apache-2.0
//! The protocol between NEOWEB.EXE (the browser UI) and NEOWEBE.EXE (the engine process: Servo), browser WB4c.
//!
//! - **Transport:** NEOWEBE runs the Symbian server [`SERVER_NAME`]. NEOWEB sends each [`Command`] with
//!   [`func::COMMAND`] (argument 0: the encoded command) and keeps one [`func::NEXT_EVENT`] outstanding (argument 0:
//!   a buffer of [`EVENT_MAX`] bytes); the engine completes it with the encoded [`Event`]'s length.
//! - **Frames:** the global chunk [`FRAME_CHUNK`] holds a [`FrameHeader`] and two BGRX buffers of the page area.
//!   [`Event::Frame`] names the buffer just written; the engine writes nothing more until
//!   [`Command::FrameShown`] returns that buffer (WB0's rule: never paint faster than frames are shown).
//!
//! Every message is a tag byte and little-endian fields; strings are a `u32` length and UTF-8.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// The engine's server name.
pub const SERVER_NAME: &str = "NeoWebEngine";
/// The protocol version (major, minor, build), checked at connect.
pub const VERSION: (u8, u8, u16) = (1, 0, 0);
/// The global chunk with the frames.
pub const FRAME_CHUNK: &str = "NeoWebFrames";
/// Room for one encoded event (URLs and titles are cut to fit).
pub const EVENT_MAX: usize = 4096;

/// Message functions.
pub mod func {
    pub const COMMAND: i32 = 1;
    pub const NEXT_EVENT: i32 = 2;
}

/// A rectangle in page pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// What a touch point does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchKind {
    Down = 0,
    Move = 1,
    Up = 2,
    Cancel = 3,
}

/// NEOWEB → engine.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Load(String),
    Reload,
    Stop,
    Back,
    Forward,
    /// The page area in pixels, and the device pixel ratio × 1000.
    Resize {
        w: u32,
        h: u32,
        dpr_milli: u32,
    },
    Touch {
        kind: TouchKind,
        id: u32,
        x: i32,
        y: i32,
    },
    /// A scroll by (dx, dy) pixels at (x, y).
    Scroll {
        dx: i32,
        dy: i32,
        x: i32,
        y: i32,
    },
    Key {
        code: u32,
        down: bool,
    },
    Text(String),
    /// NEOWEB has shown frame `seq` and gives its buffer back.
    FrameShown {
        seq: u32,
    },
}

/// Page load progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    Started = 0,
    HeadParsed = 1,
    Complete = 2,
}

/// Engine → NEOWEB.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Load(Load),
    /// Frame `seq` is in buffer `buffer` (0 or 1); `dirty` changed since the previous frame.
    Frame {
        seq: u32,
        buffer: u8,
        dirty: Rect,
    },
    Url(String),
    Title(String),
    History {
        back: bool,
        forward: bool,
    },
    /// The page (or the engine) failed; NEOWEB shows "page crashed" with Reload.
    Crashed(String),
}

/// The start of the frame chunk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameHeader {
    /// [`FrameHeader::MAGIC`] once the engine has set the chunk up.
    pub magic: u32,
    pub width: u32,
    pub height: u32,
    /// Bytes per row (BGRX, 4 bytes a pixel).
    pub stride: u32,
    /// Offsets of the two buffers from the chunk's base.
    pub offsets: [u32; 2],
}

impl FrameHeader {
    pub const MAGIC: u32 = u32::from_le_bytes(*b"NWF1");
    /// The header's room at the chunk's start (the buffers start page-aligned after it).
    pub const SIZE: usize = 4096;

    /// The layout for a `width` × `height` page: the chunk size it needs and the header.
    pub fn layout(width: u32, height: u32) -> (usize, FrameHeader) {
        let stride = width * 4;
        let buffer = (stride as usize * height as usize).next_multiple_of(4096);
        let h = FrameHeader {
            magic: Self::MAGIC,
            width,
            height,
            stride,
            offsets: [Self::SIZE as u32, (Self::SIZE + buffer) as u32],
        };
        (Self::SIZE + 2 * buffer, h)
    }
}

/// A malformed message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadMessage;

struct W(Vec<u8>);

impl W {
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(self, v: i32) -> Self {
        self.u32(v as u32)
    }
    /// At most `max` bytes of `s`, cut at a character boundary.
    fn str(self, s: &str, max: usize) -> Self {
        let mut end = s.len().min(max);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut w = self.u32(end as u32);
        w.0.extend_from_slice(&s.as_bytes()[..end]);
        w
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn u8(&mut self) -> Result<u8, BadMessage> {
        let (&v, rest) = self.0.split_first().ok_or(BadMessage)?;
        self.0 = rest;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32, BadMessage> {
        let (v, rest) = self.0.split_first_chunk::<4>().ok_or(BadMessage)?;
        self.0 = rest;
        Ok(u32::from_le_bytes(*v))
    }
    fn i32(&mut self) -> Result<i32, BadMessage> {
        self.u32().map(|v| v as i32)
    }
    fn bool(&mut self) -> Result<bool, BadMessage> {
        self.u8().map(|v| v != 0)
    }
    fn str(&mut self) -> Result<String, BadMessage> {
        let n = self.u32()? as usize;
        if n > self.0.len() {
            return Err(BadMessage);
        }
        let (s, rest) = self.0.split_at(n);
        self.0 = rest;
        core::str::from_utf8(s)
            .map(String::from)
            .map_err(|_| BadMessage)
    }
    /// The whole message was read.
    fn end<T>(self, v: T) -> Result<T, BadMessage> {
        if self.0.is_empty() {
            Ok(v)
        } else {
            Err(BadMessage)
        }
    }
}

/// Strings in commands are URLs and typed text: 64 KiB is plenty.
const COMMAND_STR_MAX: usize = 65536;
/// Strings in events must leave room for the tag and length in [`EVENT_MAX`].
const EVENT_STR_MAX: usize = EVENT_MAX - 8;

impl Command {
    pub fn encode(&self) -> Vec<u8> {
        let w = W(Vec::new());
        match self {
            Command::Load(url) => w.u8(0).str(url, COMMAND_STR_MAX),
            Command::Reload => w.u8(1),
            Command::Stop => w.u8(2),
            Command::Back => w.u8(3),
            Command::Forward => w.u8(4),
            Command::Resize {
                w: width,
                h,
                dpr_milli,
            } => w.u8(5).u32(*width).u32(*h).u32(*dpr_milli),
            Command::Touch { kind, id, x, y } => w.u8(6).u8(*kind as u8).u32(*id).i32(*x).i32(*y),
            Command::Scroll { dx, dy, x, y } => w.u8(7).i32(*dx).i32(*dy).i32(*x).i32(*y),
            Command::Key { code, down } => w.u8(8).u32(*code).u8(*down as u8),
            Command::Text(t) => w.u8(9).str(t, COMMAND_STR_MAX),
            Command::FrameShown { seq } => w.u8(10).u32(*seq),
        }
        .0
    }

    pub fn decode(b: &[u8]) -> Result<Command, BadMessage> {
        let mut r = R(b);
        let c = match r.u8()? {
            0 => Command::Load(r.str()?),
            1 => Command::Reload,
            2 => Command::Stop,
            3 => Command::Back,
            4 => Command::Forward,
            5 => Command::Resize {
                w: r.u32()?,
                h: r.u32()?,
                dpr_milli: r.u32()?,
            },
            6 => Command::Touch {
                kind: match r.u8()? {
                    0 => TouchKind::Down,
                    1 => TouchKind::Move,
                    2 => TouchKind::Up,
                    3 => TouchKind::Cancel,
                    _ => return Err(BadMessage),
                },
                id: r.u32()?,
                x: r.i32()?,
                y: r.i32()?,
            },
            7 => Command::Scroll {
                dx: r.i32()?,
                dy: r.i32()?,
                x: r.i32()?,
                y: r.i32()?,
            },
            8 => Command::Key {
                code: r.u32()?,
                down: r.bool()?,
            },
            9 => Command::Text(r.str()?),
            10 => Command::FrameShown { seq: r.u32()? },
            _ => return Err(BadMessage),
        };
        r.end(c)
    }
}

impl Event {
    /// At most [`EVENT_MAX`] bytes.
    pub fn encode(&self) -> Vec<u8> {
        let w = W(Vec::new());
        match self {
            Event::Load(l) => w.u8(0).u8(*l as u8),
            Event::Frame { seq, buffer, dirty } => w
                .u8(1)
                .u32(*seq)
                .u8(*buffer)
                .i32(dirty.x)
                .i32(dirty.y)
                .i32(dirty.w)
                .i32(dirty.h),
            Event::Url(u) => w.u8(2).str(u, EVENT_STR_MAX),
            Event::Title(t) => w.u8(3).str(t, EVENT_STR_MAX),
            Event::History { back, forward } => w.u8(4).u8(*back as u8).u8(*forward as u8),
            Event::Crashed(why) => w.u8(5).str(why, EVENT_STR_MAX),
        }
        .0
    }

    pub fn decode(b: &[u8]) -> Result<Event, BadMessage> {
        let mut r = R(b);
        let e = match r.u8()? {
            0 => Event::Load(match r.u8()? {
                0 => Load::Started,
                1 => Load::HeadParsed,
                2 => Load::Complete,
                _ => return Err(BadMessage),
            }),
            1 => Event::Frame {
                seq: r.u32()?,
                buffer: match r.u8()? {
                    b @ (0 | 1) => b,
                    _ => return Err(BadMessage),
                },
                dirty: Rect {
                    x: r.i32()?,
                    y: r.i32()?,
                    w: r.i32()?,
                    h: r.i32()?,
                },
            },
            2 => Event::Url(r.str()?),
            3 => Event::Title(r.str()?),
            4 => Event::History {
                back: r.bool()?,
                forward: r.bool()?,
            },
            5 => Event::Crashed(r.str()?),
            _ => return Err(BadMessage),
        };
        r.end(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn commands_round_trip() {
        let all = vec![
            Command::Load("https://en.wikipedia.org/wiki/Яблоко".to_string()),
            Command::Reload,
            Command::Stop,
            Command::Back,
            Command::Forward,
            Command::Resize {
                w: 1080,
                h: 2037,
                dpr_milli: 2750,
            },
            Command::Touch {
                kind: TouchKind::Move,
                id: 3,
                x: -5,
                y: 2000,
            },
            Command::Scroll {
                dx: 0,
                dy: -120,
                x: 540,
                y: 900,
            },
            Command::Key {
                code: 0x0d,
                down: true,
            },
            Command::Text("Բարեւ".to_string()),
            Command::FrameShown { seq: 7 },
        ];
        for c in all {
            assert_eq!(Command::decode(&c.encode()), Ok(c));
        }
    }

    #[test]
    fn events_round_trip() {
        let all = vec![
            Event::Load(Load::HeadParsed),
            Event::Frame {
                seq: 9,
                buffer: 1,
                dirty: Rect {
                    x: 0,
                    y: 10,
                    w: 1080,
                    h: 200,
                },
            },
            Event::Url("file:///c:/pages/a.html".to_string()),
            Event::Title("Test".to_string()),
            Event::History {
                back: true,
                forward: false,
            },
            Event::Crashed("out of memory".to_string()),
        ];
        for e in all {
            assert_eq!(Event::decode(&e.encode()), Ok(e));
        }
    }

    #[test]
    fn long_event_strings_are_cut_to_fit_at_a_char_boundary() {
        let title = "ж".repeat(EVENT_MAX); // 2 bytes a character
        let b = Event::Title(title).encode();
        assert!(b.len() <= EVENT_MAX);
        let Ok(Event::Title(t)) = Event::decode(&b) else {
            panic!()
        };
        assert!(t.len() <= EVENT_STR_MAX && t.chars().all(|c| c == 'ж'));
    }

    #[test]
    fn malformed_messages_are_rejected() {
        assert_eq!(Command::decode(&[]), Err(BadMessage));
        assert_eq!(Command::decode(&[99]), Err(BadMessage));
        assert_eq!(Command::decode(&[0, 200, 0, 0, 0, b'a']), Err(BadMessage)); // length past the end
        assert_eq!(Command::decode(&[1, 0]), Err(BadMessage)); // trailing byte
        assert_eq!(
            Event::decode(&[1, 0, 0, 0, 0, 2, 0, 0, 0, 0]),
            Err(BadMessage)
        ); // buffer 2
        assert_eq!(Event::decode(&[3, 2, 0, 0, 0, 0xff, 0xfe]), Err(BadMessage));
        // not UTF-8
    }

    #[test]
    fn frame_layout_has_two_aligned_buffers_after_the_header() {
        let (size, h) = FrameHeader::layout(1080, 2037);
        assert_eq!(h.stride, 4320);
        assert_eq!(h.offsets[0] as usize, FrameHeader::SIZE);
        assert_eq!(h.offsets[0] % 4096, 0);
        assert_eq!(h.offsets[1] % 4096, 0);
        assert!(h.offsets[1] as usize >= h.offsets[0] as usize + 4320 * 2037);
        assert!(size >= h.offsets[1] as usize + 4320 * 2037);
    }
}
