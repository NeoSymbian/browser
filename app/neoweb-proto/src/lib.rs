// SPDX-License-Identifier: Apache-2.0
//! The protocol between NEOWEB.EXE (the browser UI) and NEOWEBE.EXE (the engine process: Servo), browser WB4c.
//!
//! - **Transport:** NEOWEBE runs the Symbian server [`SERVER_NAME`]. NEOWEB sends each [`Command`] with
//!   [`func::COMMAND`] (argument 0: the encoded command) and keeps one [`func::NEXT_EVENT`] outstanding (argument 0:
//!   a buffer of [`EVENT_MAX`] bytes); the engine completes it with the encoded [`Event`]'s length.
//! - **Frames:** an unnamed global chunk holds a [`FrameHeader`] and two buffers sized for the largest page area
//!   (RGBA8, opaque, rows top-down: what Slint's `SharedPixelBuffer<Rgba8Pixel>` takes with one copy). NEOWEB asks
//!   for it once with [`func::FRAMES`], which the engine completes with a handle to the chunk.
//!   [`Event::Frame`] names the buffer just written and the page size in it; the engine writes nothing more into
//!   that buffer until [`Command::FrameShown`] returns it (WB0's rule: never paint faster than frames are shown).
//! - **Ending:** NEOWEB sends [`func::CANCEL`] before it drops its held [`func::NEXT_EVENT`] (a pending request
//!   can't be withdrawn); when the engine dies the held request fails with `KErrServerTerminated`.
//!
//! - **Controls (1.1, WB5a):** a focused field asks for the keyboard ([`Event::Keyboard`]); script dialogs, page
//!   unloads, `<select>` and `<input type=file>` arrive as [`Event::Dialog`], [`Event::Select`] and
//!   [`Event::FilePick`] with an id, answered once with [`Command::Answer`], [`Command::Choose`] and
//!   [`Command::Files`]. The page's clipboard is the shell's: [`Event::ClipboardSet`], and [`Event::ClipboardGet`]
//!   answered with [`Command::Clipboard`].
//!
//! - **Tabs (1.2, WB5c):** each opened page is a tab with NEOWEB's id: [`Command::Open`] makes (or reloads) its page
//!   and shows it, [`Command::Activate`] shows another, [`Command::Close`] frees one (closed or evicted). Load, Url,
//!   Title and History are the shown tab's (sent again on activation); the others report [`Event::TabInfo`]. A
//!   page's own new window comes as [`Event::Created`] with an engine id (>= [`ENGINE_TABS`]).
//! - **Long-press, selection, find (1.2):** [`Command::Press`] → [`Event::Pressed`] (a link, an image, or a word
//!   selected); [`Command::Select`] and [`Command::Find`] → [`Event::Selection`] / [`Event::Found`].
//!
//! - **Downloads (1.3, WB5d):** [`Command::Download`] (the link menu) or a page's download link starts one in the
//!   engine, into `C:\Data\Downloads`; [`Event::Download`] reports it; [`Command::DownloadOp`] pauses, resumes or
//!   cancels it.
//!
//! Every message is a tag byte and little-endian fields; strings are a `u32` length and UTF-8; lists a `u32` count.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// The engine's server name.
pub const SERVER_NAME: &str = "NeoWebEngine";
/// The protocol version (major, minor, build), checked at connect.
pub const VERSION: (u8, u8, u16) = (1, 4, 0);
/// Tab ids from here up are the engine's (a page's new window); below, NEOWEB's.
pub const ENGINE_TABS: u32 = 0x8000_0000;
/// Room for one encoded event (URLs and titles are cut to fit).
pub const EVENT_MAX: usize = 4096;

/// Message functions.
pub mod func {
    pub const COMMAND: i32 = 1;
    pub const NEXT_EVENT: i32 = 2;
    /// Completes with a handle to the frame chunk.
    pub const FRAMES: i32 = 3;
    /// Completes the session's held NEXT_EVENT with `KErrCancel` (-3).
    pub const CANCEL: i32 = 4;
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
    /// A key from the shell for the focused page field: a Unicode scalar, Backspace 8, Enter 13, or Ctrl 0x11 before
    /// `a`, `c` or `v` (the keyboard's edit bar). The keyboard sends presses only (`down`).
    Key {
        code: u32,
        down: bool,
    },
    Text(String),
    /// NEOWEB has shown frame `seq` and gives its buffer back.
    FrameShown {
        seq: u32,
    },
    /// The user hid the keyboard the page asked for.
    ImeDismissed,
    /// The answer to [`Event::Dialog`] `id`: OK (or Leave) or Cancel, and a prompt's text.
    Answer {
        id: u32,
        ok: bool,
        text: String,
    },
    /// The answer to [`Event::Select`] `id`: the chosen [`SelectItem::id`]s, or `cancel` (keep the old choice).
    Choose {
        id: u32,
        cancel: bool,
        picked: Vec<u32>,
    },
    /// The answer to [`Event::FilePick`] `id`: files the engine can read; none = cancelled.
    Files {
        id: u32,
        paths: Vec<String>,
    },
    /// The answer to [`Event::ClipboardGet`] `id`.
    Clipboard {
        id: u32,
        text: String,
    },
    /// Show tab `tab` with `url` loaded (its page is made if it has none: a new or evicted tab).
    Open {
        tab: u32,
        url: String,
    },
    /// Show tab `tab` (it has a page).
    Activate {
        tab: u32,
    },
    /// Free tab `tab`'s page (closed, or evicted to save memory).
    Close {
        tab: u32,
    },
    /// A long-press at (x, y) page pixels: answered with [`Event::Pressed`].
    Press {
        x: i32,
        y: i32,
    },
    /// The text selection: [`SelectOp`], the point for a handle drag.
    Select {
        op: SelectOp,
        x: i32,
        y: i32,
    },
    /// Find `text` from the top (`dir` 0), the next (1) or previous (-1) match; empty `text` stops.
    Find {
        text: String,
        dir: i32,
    },
    /// The desktop site (a desktop user agent) for the pages loaded from now.
    DesktopSite(bool),
    /// Download `url` (the link menu's Download link).
    Download {
        url: String,
    },
    /// Pause, resume or cancel download `id`.
    DownloadOp {
        id: u32,
        op: DownloadOp,
    },
    /// The pages' zoom in percent (Settings' text size, 1.4).
    Zoom(u32),
    /// Clear browsing data: cookies, site storage and the cache (1.4).
    ClearData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadOp {
    Pause = 0,
    Resume = 1,
    Cancel = 2,
}

/// Where a download is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadState {
    Downloading = 0,
    Paused = 1,
    Done = 2,
    Failed = 3,
    Cancelled = 4,
}

/// What [`Command::Select`] does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectOp {
    /// Copy the selection (to the clipboard: [`Event::ClipboardSet`]).
    Copy = 0,
    SelectAll = 1,
    Clear = 2,
    /// Move the selection's start, or end, to the point.
    DragStart = 3,
    DragEnd = 4,
}

/// What a long-press hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressKind {
    Nothing = 0,
    Link = 1,
    Image = 2,
    /// A word is selected now ([`Event::Selection`] follows).
    Text = 3,
}

/// What a script dialog is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Alert = 0,
    Confirm = 1,
    Prompt = 2,
    /// "Leave this page?" (`beforeunload`): OK leaves.
    Unload = 3,
}

/// One row of a `<select>`'s picker: an option, or a group's label ([`SelectItem::GROUP`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectItem {
    pub id: u32,
    pub label: String,
    pub disabled: bool,
    pub selected: bool,
}

impl SelectItem {
    /// The id of a group label row (not selectable).
    pub const GROUP: u32 = u32::MAX;
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
    /// Frame `seq` is in buffer `buffer` (0 or 1), `width` × `height` pixels (rows [`FrameHeader::stride`] apart);
    /// `dirty` changed since the previous frame.
    Frame {
        seq: u32,
        buffer: u8,
        width: u32,
        height: u32,
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
    /// The keyboard the focused field wants: a neoui `KEYBOARD` mode (0 = none, 1 text, 3 number, 4 phone).
    Keyboard {
        mode: u8,
    },
    /// A script dialog or a page unload to confirm; `value` is a prompt's default text.
    Dialog {
        id: u32,
        kind: DialogKind,
        text: String,
        value: String,
    },
    /// A `<select>` was tapped: its rows (cut to fit [`EVENT_MAX`]).
    Select {
        id: u32,
        multiple: bool,
        items: Vec<SelectItem>,
    },
    /// An `<input type=file>` was tapped; `accept` lists the extensions it takes (comma-separated, empty = any).
    FilePick {
        id: u32,
        multiple: bool,
        accept: String,
    },
    /// The page copied text.
    ClipboardSet(String),
    /// The page wants the clipboard's text: answer with [`Command::Clipboard`].
    ClipboardGet {
        id: u32,
    },
    /// A form with a password was sent: NEOWEB offers Save password (br2-login-save) for the page's site.
    Login {
        user: String,
    },
    /// A tab that isn't shown changed its address or title.
    TabInfo {
        tab: u32,
        url: String,
        title: String,
    },
    /// A page opened a new window (`target=_blank`, `window.open`): tab `tab` (an engine id), shown now.
    Created {
        tab: u32,
        opener: u32,
    },
    /// What a long-press hit; `y` is the bottom of the link or image (page px) for the menu.
    Pressed {
        kind: PressKind,
        title: String,
        url: String,
        y: i32,
    },
    /// The selection's ends (page px): the start's left and top, the end's right and top, the line height; empty
    /// = nothing selected.
    Selection {
        ends: Vec<i32>,
    },
    /// Find's current match (1-based, 0 = none) of `count`.
    Found {
        index: u32,
        count: u32,
    },
    /// Download `id`: its file `name` at `path` (on C:), `received` of `total` bytes (0 = unknown), its state.
    Download {
        id: u32,
        name: String,
        /// where it comes from (for a retry)
        url: String,
        path: String,
        received: u64,
        total: u64,
        state: DownloadState,
    },
}

/// The start of the frame chunk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameHeader {
    /// [`FrameHeader::MAGIC`] once the engine has set the chunk up.
    pub magic: u32,
    /// The largest frame the buffers hold.
    pub width: u32,
    pub height: u32,
    /// Bytes per row (RGBA8, 4 bytes a pixel).
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
    fn u64(mut self, v: u64) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
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
    fn u64(&mut self) -> Result<u64, BadMessage> {
        let (v, rest) = self.0.split_first_chunk::<8>().ok_or(BadMessage)?;
        self.0 = rest;
        Ok(u64::from_le_bytes(*v))
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
    /// A `u32` count, then that many items (the count can't promise more bytes than are left).
    fn list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T, BadMessage>,
    ) -> Result<Vec<T>, BadMessage> {
        let n = self.u32()? as usize;
        if n > self.0.len() {
            return Err(BadMessage);
        }
        (0..n).map(|_| item(self)).collect()
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
            Command::ImeDismissed => w.u8(11),
            Command::Answer { id, ok, text } => {
                w.u8(12).u32(*id).u8(*ok as u8).str(text, COMMAND_STR_MAX)
            }
            Command::Choose { id, cancel, picked } => {
                let mut w = w.u8(13).u32(*id).u8(*cancel as u8).u32(picked.len() as u32);
                for p in picked {
                    w = w.u32(*p);
                }
                w
            }
            Command::Files { id, paths } => {
                let mut w = w.u8(14).u32(*id).u32(paths.len() as u32);
                for p in paths {
                    w = w.str(p, COMMAND_STR_MAX);
                }
                w
            }
            Command::Clipboard { id, text } => w.u8(15).u32(*id).str(text, COMMAND_STR_MAX),
            Command::Open { tab, url } => w.u8(16).u32(*tab).str(url, COMMAND_STR_MAX),
            Command::Activate { tab } => w.u8(17).u32(*tab),
            Command::Close { tab } => w.u8(18).u32(*tab),
            Command::Press { x, y } => w.u8(19).i32(*x).i32(*y),
            Command::Select { op, x, y } => w.u8(20).u8(*op as u8).i32(*x).i32(*y),
            Command::Find { text, dir } => w.u8(21).str(text, 1024).i32(*dir),
            Command::DesktopSite(on) => w.u8(22).u8(*on as u8),
            Command::Download { url } => w.u8(23).str(url, COMMAND_STR_MAX),
            Command::DownloadOp { id, op } => w.u8(24).u32(*id).u8(*op as u8),
            Command::Zoom(p) => w.u8(25).u32(*p),
            Command::ClearData => w.u8(26),
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
            11 => Command::ImeDismissed,
            12 => Command::Answer {
                id: r.u32()?,
                ok: r.bool()?,
                text: r.str()?,
            },
            13 => Command::Choose {
                id: r.u32()?,
                cancel: r.bool()?,
                picked: r.list(|r| r.u32())?,
            },
            14 => Command::Files {
                id: r.u32()?,
                paths: r.list(|r| r.str())?,
            },
            15 => Command::Clipboard {
                id: r.u32()?,
                text: r.str()?,
            },
            16 => Command::Open {
                tab: r.u32()?,
                url: r.str()?,
            },
            17 => Command::Activate { tab: r.u32()? },
            18 => Command::Close { tab: r.u32()? },
            19 => Command::Press {
                x: r.i32()?,
                y: r.i32()?,
            },
            20 => Command::Select {
                op: match r.u8()? {
                    0 => SelectOp::Copy,
                    1 => SelectOp::SelectAll,
                    2 => SelectOp::Clear,
                    3 => SelectOp::DragStart,
                    4 => SelectOp::DragEnd,
                    _ => return Err(BadMessage),
                },
                x: r.i32()?,
                y: r.i32()?,
            },
            21 => Command::Find {
                text: r.str()?,
                dir: r.i32()?,
            },
            22 => Command::DesktopSite(r.bool()?),
            23 => Command::Download { url: r.str()? },
            25 => Command::Zoom(r.u32()?),
            26 => Command::ClearData,
            24 => Command::DownloadOp {
                id: r.u32()?,
                op: match r.u8()? {
                    0 => DownloadOp::Pause,
                    1 => DownloadOp::Resume,
                    2 => DownloadOp::Cancel,
                    _ => return Err(BadMessage),
                },
            },
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
            Event::Frame {
                seq,
                buffer,
                width,
                height,
                dirty,
            } => w
                .u8(1)
                .u32(*seq)
                .u8(*buffer)
                .u32(*width)
                .u32(*height)
                .i32(dirty.x)
                .i32(dirty.y)
                .i32(dirty.w)
                .i32(dirty.h),
            Event::Url(u) => w.u8(2).str(u, EVENT_STR_MAX),
            Event::Title(t) => w.u8(3).str(t, EVENT_STR_MAX),
            Event::History { back, forward } => w.u8(4).u8(*back as u8).u8(*forward as u8),
            Event::Crashed(why) => w.u8(5).str(why, EVENT_STR_MAX),
            Event::Keyboard { mode } => w.u8(6).u8(*mode),
            Event::Dialog {
                id,
                kind,
                text,
                value,
            } => {
                // the text first: a prompt's default value takes what is left
                let w = w
                    .u8(7)
                    .u32(*id)
                    .u8(*kind as u8)
                    .str(text, EVENT_STR_MAX / 2);
                let left = EVENT_MAX - w.0.len() - 4;
                w.str(value, left)
            }
            Event::Select {
                id,
                multiple,
                items,
            } => {
                let head = w.u8(8).u32(*id).u8(*multiple as u8);
                let mut rows = Vec::new();
                let mut n = 0u32;
                for it in items {
                    let row = W(Vec::new())
                        .u32(it.id)
                        .u8(it.disabled as u8)
                        .u8(it.selected as u8)
                        .str(&it.label, 256)
                        .0;
                    // shortcut: a long list is cut to fit one event; page it if a real site needs more
                    if head.0.len() + 4 + rows.len() + row.len() > EVENT_MAX {
                        break;
                    }
                    rows.extend_from_slice(&row);
                    n += 1;
                }
                let mut w = head.u32(n);
                w.0.extend_from_slice(&rows);
                w
            }
            Event::FilePick {
                id,
                multiple,
                accept,
            } => w.u8(9).u32(*id).u8(*multiple as u8).str(accept, 1024),
            Event::ClipboardSet(t) => w.u8(10).str(t, EVENT_STR_MAX),
            Event::ClipboardGet { id } => w.u8(11).u32(*id),
            Event::Login { user } => w.u8(12).str(user, 256),
            Event::TabInfo { tab, url, title } => w
                .u8(13)
                .u32(*tab)
                .str(url, EVENT_STR_MAX / 2)
                .str(title, 512),
            Event::Created { tab, opener } => w.u8(14).u32(*tab).u32(*opener),
            Event::Pressed {
                kind,
                title,
                url,
                y,
            } => w
                .u8(15)
                .u8(*kind as u8)
                .str(title, 256)
                .str(url, EVENT_STR_MAX / 2)
                .i32(*y),
            Event::Selection { ends } => {
                let mut w = w.u8(16).u32(ends.len().min(8) as u32);
                for e in ends.iter().take(8) {
                    w = w.i32(*e);
                }
                w
            }
            Event::Found { index, count } => w.u8(17).u32(*index).u32(*count),
            Event::Download {
                id,
                name,
                url,
                path,
                received,
                total,
                state,
            } => w
                .u8(18)
                .u32(*id)
                .str(name, 512)
                .str(url, 2048)
                .str(path, 1024)
                .u64(*received)
                .u64(*total)
                .u8(*state as u8),
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
                width: r.u32()?,
                height: r.u32()?,
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
            6 => Event::Keyboard { mode: r.u8()? },
            7 => Event::Dialog {
                id: r.u32()?,
                kind: match r.u8()? {
                    0 => DialogKind::Alert,
                    1 => DialogKind::Confirm,
                    2 => DialogKind::Prompt,
                    3 => DialogKind::Unload,
                    _ => return Err(BadMessage),
                },
                text: r.str()?,
                value: r.str()?,
            },
            8 => Event::Select {
                id: r.u32()?,
                multiple: r.bool()?,
                items: r.list(|r| {
                    Ok(SelectItem {
                        id: r.u32()?,
                        disabled: r.bool()?,
                        selected: r.bool()?,
                        label: r.str()?,
                    })
                })?,
            },
            9 => Event::FilePick {
                id: r.u32()?,
                multiple: r.bool()?,
                accept: r.str()?,
            },
            10 => Event::ClipboardSet(r.str()?),
            11 => Event::ClipboardGet { id: r.u32()? },
            12 => Event::Login { user: r.str()? },
            13 => Event::TabInfo {
                tab: r.u32()?,
                url: r.str()?,
                title: r.str()?,
            },
            14 => Event::Created {
                tab: r.u32()?,
                opener: r.u32()?,
            },
            15 => Event::Pressed {
                kind: match r.u8()? {
                    0 => PressKind::Nothing,
                    1 => PressKind::Link,
                    2 => PressKind::Image,
                    3 => PressKind::Text,
                    _ => return Err(BadMessage),
                },
                title: r.str()?,
                url: r.str()?,
                y: r.i32()?,
            },
            16 => Event::Selection {
                ends: r.list(|r| r.i32())?,
            },
            17 => Event::Found {
                index: r.u32()?,
                count: r.u32()?,
            },
            18 => Event::Download {
                id: r.u32()?,
                name: r.str()?,
                url: r.str()?,
                path: r.str()?,
                received: r.u64()?,
                total: r.u64()?,
                state: match r.u8()? {
                    0 => DownloadState::Downloading,
                    1 => DownloadState::Paused,
                    2 => DownloadState::Done,
                    3 => DownloadState::Failed,
                    4 => DownloadState::Cancelled,
                    _ => return Err(BadMessage),
                },
            },
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
            Command::ImeDismissed,
            Command::Answer {
                id: 4,
                ok: true,
                text: "Ереван".to_string(),
            },
            Command::Choose {
                id: 5,
                cancel: false,
                picked: vec![0, 3],
            },
            Command::Files {
                id: 6,
                paths: vec!["C:/Private/e0007008/upload/a.jpg".to_string()],
            },
            Command::Clipboard {
                id: 1,
                text: "copied".to_string(),
            },
            Command::Open {
                tab: 3,
                url: "https://github.com/NeoSymbian".to_string(),
            },
            Command::Activate { tab: 2 },
            Command::Close { tab: 1 },
            Command::Press { x: 500, y: 900 },
            Command::Select {
                op: SelectOp::DragEnd,
                x: 10,
                y: -4,
            },
            Command::Find {
                text: "Symbian".to_string(),
                dir: -1,
            },
            Command::DesktopSite(true),
            Command::Download {
                url: "https://example.org/a.pdf".to_string(),
            },
            Command::DownloadOp {
                id: 3,
                op: DownloadOp::Resume,
            },
            Command::Zoom(115),
            Command::ClearData,
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
                width: 1080,
                height: 2037,
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
            Event::Keyboard { mode: 3 },
            Event::Dialog {
                id: 2,
                kind: DialogKind::Prompt,
                text: "Your name?".to_string(),
                value: "Anna".to_string(),
            },
            Event::Select {
                id: 3,
                multiple: false,
                items: vec![
                    SelectItem {
                        id: SelectItem::GROUP,
                        label: "Fruit".to_string(),
                        disabled: false,
                        selected: false,
                    },
                    SelectItem {
                        id: 0,
                        label: "Apricot".to_string(),
                        disabled: false,
                        selected: true,
                    },
                ],
            },
            Event::FilePick {
                id: 4,
                multiple: true,
                accept: "jpg,png".to_string(),
            },
            Event::ClipboardSet("text".to_string()),
            Event::ClipboardGet { id: 9 },
            Event::Login {
                user: "ava@example.org".to_string(),
            },
            Event::TabInfo {
                tab: 2,
                url: "https://slint.dev/docs".to_string(),
                title: "Slint docs".to_string(),
            },
            Event::Created {
                tab: ENGINE_TABS + 1,
                opener: 2,
            },
            Event::Pressed {
                kind: PressKind::Link,
                title: "Eclipse Public License".to_string(),
                url: "https://en.wikipedia.org/wiki/Eclipse_Public_License".to_string(),
                y: 960,
            },
            Event::Selection {
                ends: vec![30, 600, 900, 660, 51],
            },
            Event::Selection { ends: vec![] },
            Event::Found { index: 2, count: 7 },
            Event::Download {
                id: 1,
                name: "symbian3-dev-guide.pdf".to_string(),
                url: "https://example.org/symbian3-dev-guide.pdf".to_string(),
                path: "C:/Data/Downloads/symbian3-dev-guide.pdf".to_string(),
                received: 3_355_443,
                total: 8_808_038,
                state: DownloadState::Downloading,
            },
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
    fn long_selects_and_dialogs_fit_one_event() {
        let items = (0..500)
            .map(|i| SelectItem {
                id: i,
                label: alloc::format!("Option number {i}"),
                disabled: false,
                selected: i == 0,
            })
            .collect();
        let b = Event::Select {
            id: 1,
            multiple: false,
            items,
        }
        .encode();
        assert!(b.len() <= EVENT_MAX);
        let Ok(Event::Select { items, .. }) = Event::decode(&b) else {
            panic!()
        };
        assert!(items.len() > 50 && items[0].selected);
        let b = Event::Dialog {
            id: 1,
            kind: DialogKind::Prompt,
            text: "a".repeat(EVENT_MAX),
            value: "b".repeat(EVENT_MAX),
        }
        .encode();
        assert!(b.len() <= EVENT_MAX);
        assert!(Event::decode(&b).is_ok());
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
        // not UTF-8
        assert_eq!(Event::decode(&[3, 2, 0, 0, 0, 0xff, 0xfe]), Err(BadMessage));
        // a huge count
        assert_eq!(
            Command::decode(&[13, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]),
            Err(BadMessage)
        );
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
