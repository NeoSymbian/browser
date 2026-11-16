// SPDX-License-Identifier: Apache-2.0
//! Servo → harness notifications: load status, frames, console output, crashes.

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::Write;
use std::time::Instant;

use servo::{ConsoleLogLevel, LoadStatus, WebView, WebViewDelegate};

pub struct State {
    pub start: Instant,
    /// Load-complete events seen so far; a navigation waits until this passes its mark.
    pub loads_complete: Cell<u32>,
    /// Servo has a new frame to paint.
    pub frame_ready: Cell<bool>,
    /// Frames painted so far.
    pub frames: Cell<u64>,
    /// Last paint (or step start, see `reset_gap`), for the longest gap between frames.
    last_frame: Cell<Instant>,
    pub max_gap_ms: Cell<u64>,
    pub crashed: RefCell<Option<String>>,
    console: RefCell<Option<File>>,
}

impl State {
    pub fn new(console: Option<File>) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            loads_complete: Cell::new(0),
            frame_ready: Cell::new(false),
            frames: Cell::new(0),
            last_frame: Cell::new(now),
            max_gap_ms: Cell::new(0),
            crashed: RefCell::new(None),
            console: RefCell::new(console),
        }
    }

    /// Milliseconds since start.
    pub fn ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    pub fn painted(&self, now: Instant) {
        self.frames.set(self.frames.get() + 1);
        let gap = now.duration_since(self.last_frame.get()).as_millis() as u64;
        self.max_gap_ms.set(self.max_gap_ms.get().max(gap));
        self.last_frame.set(now);
    }

    /// Measure frame gaps from now on (called at each step start).
    pub fn reset_gap(&self, now: Instant) {
        self.last_frame.set(now);
        self.max_gap_ms.set(0);
    }
}

impl WebViewDelegate for State {
    fn notify_load_status_changed(&self, _: WebView, status: LoadStatus) {
        if matches!(status, LoadStatus::Complete) {
            self.loads_complete.set(self.loads_complete.get() + 1);
        }
        eprintln!("[{:>7}] load {status:?}", self.ms());
    }

    fn notify_new_frame_ready(&self, _: WebView) {
        self.frame_ready.set(true);
    }

    fn notify_animating_changed(&self, _: WebView, animating: bool) {
        eprintln!("[{:>7}] animating={animating}", self.ms());
    }

    fn show_console_message(&self, _: WebView, level: ConsoleLogLevel, message: String) {
        if let Some(file) = self.console.borrow_mut().as_mut() {
            let _ = writeln!(file, "{} {level:?} {message}", self.ms());
        }
    }

    fn notify_crashed(&self, _: WebView, reason: String, backtrace: Option<String>) {
        *self.crashed.borrow_mut() = Some(format!("{reason}\n{}", backtrace.unwrap_or_default()));
    }
}
