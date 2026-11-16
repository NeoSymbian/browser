// SPDX-License-Identifier: Apache-2.0
//! Runs a workflow's steps one at a time against the webview; `poll` is called from the event loop.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use euclid::Point2D;
use servo::profile_traits::mem::MemoryReportResult;
use servo::{
    Code, DeviceIntRect, InputEvent, JSValue, Key, KeyState, KeyboardEvent, Location, Modifiers,
    NamedKey, RenderingContext, Servo, TouchEvent, TouchEventType, TouchId, TouchPointerType,
    WebView, WebViewPoint, WheelDelta, WheelEvent, WheelMode,
};
use servo_base::generic_channel::GenericCallback;
use wb0_workflow::{Action, Workflow, expand};

use crate::delegate::State;
use crate::out::{Out, Row};

/// How often waits re-evaluate their JavaScript condition.
const JS_POLL: Duration = Duration::from_millis(250);
/// A scroll step ends once no frame has been painted for this long.
const SCROLL_QUIET: Duration = Duration::from_millis(600);
/// …or at the latest after this long, for pages that animate continuously.
const SCROLL_MAX: Duration = Duration::from_secs(5);
/// …and gives up waiting for a frame of the chunk after this long.
const SCROLL_NO_FRAME: Duration = Duration::from_secs(60);
/// Time given to input to take effect before the next step.
const INPUT_SETTLE: Duration = Duration::from_millis(400);

pub struct Cx<'a> {
    pub servo: &'a Servo,
    pub webview: &'a WebView,
    pub ctx: &'a Rc<dyn RenderingContext>,
    pub state: &'a State,
}

pub enum Poll {
    Pending,
    /// Exit code: 0 all required steps passed, 1 a required step failed.
    Done(u8),
}

enum Wait {
    /// The page started by the last navigation is load-complete.
    Load,
    /// `expr` evaluates truthy; for a tap it returns the target point `[x, y]` instead.
    Js {
        expr: String,
        tap: bool,
    },
    /// A scroll chunk is done when no frame came for `SCROLL_QUIET` (or after `SCROLL_MAX`); then the next chunk of
    /// the `left` CSS pixels is sent, at most `chunk` at a time.
    Quiet {
        frames: u64,
        since: Instant,
        chunk_start: Instant,
        left: f32,
        chunk: f32,
        painted_at_start: u64,
    },
    Until(Instant),
    Mem(mpsc::Receiver<MemoryReportResult>, String),
}

type JsResult = Rc<RefCell<Option<Result<JSValue, String>>>>;

pub struct Exec {
    wf: Workflow,
    soak_until: Option<Instant>,
    iteration: u32,
    idx: usize,
    wait: Option<Wait>,
    step_start: Instant,
    step_t0: u64,
    frames0: u64,
    nav_mark: u32,
    js: JsResult,
    js_inflight: bool,
    next_js: Instant,
    failed: bool,
}

impl Exec {
    pub fn new(wf: Workflow, soak: Option<Duration>) -> Self {
        let now = Instant::now();
        Self {
            wf,
            soak_until: soak.map(|d| now + d),
            iteration: 0,
            idx: 0,
            wait: None,
            step_start: now,
            step_t0: 0,
            frames0: 0,
            nav_mark: 0, // the first page is loaded by the WebViewBuilder
            js: Rc::default(),
            js_inflight: false,
            next_js: now,
            failed: false,
        }
    }

    pub fn poll(&mut self, cx: &Cx, out: &mut Out) -> Poll {
        let now = Instant::now();
        if self.idx == self.wf.steps.len() {
            match self.soak_until {
                Some(until) if now < until => self.restart(cx),
                _ => return Poll::Done(self.failed as u8),
            }
        }
        if self.wait.is_none() {
            self.step_start = now;
            self.step_t0 = cx.state.ms();
            self.frames0 = cx.state.frames.get();
            cx.state.reset_gap(now);
            match self.begin(cx, out) {
                Ok(wait) => self.wait = wait,
                Err(note) => return self.finish(cx, out, false, &note),
            }
        }
        let Some(wait) = self.wait.take() else {
            return self.finish(cx, out, true, "");
        };
        match self.check(cx, out, now, wait) {
            Ok(None) => self.finish(cx, out, true, ""),
            Ok(Some(wait)) => {
                let timeout = Duration::from_millis(self.wf.steps[self.idx].timeout_ms());
                if now.duration_since(self.step_start) > timeout {
                    return self.finish(cx, out, false, "timeout");
                }
                self.wait = Some(wait);
                Poll::Pending
            }
            Err(note) => self.finish(cx, out, false, &note),
        }
    }

    /// Start the current step; `None` means it is already complete.
    fn begin(&mut self, cx: &Cx, out: &mut Out) -> Result<Option<Wait>, String> {
        let env = |k: &str| std::env::var(k).ok();
        let action = self.wf.steps[self.idx].action.clone();
        Ok(match action {
            Action::Nav { url } => {
                self.navigate(cx, &expand(&url, env)?)?;
                None
            }
            Action::WaitLoad => Some(Wait::Load),
            Action::WaitSel { sel } => Some(Wait::Js {
                expr: format!("!!document.querySelector({})", quote(&sel)),
                tap: false,
            }),
            Action::WaitJs { js } => Some(Wait::Js {
                expr: format!("!!({js})"),
                tap: false,
            }),
            Action::Tap { sel } => Some(Wait::Js {
                expr: format!(
                    "(() => {{ const e = document.querySelector({}); if (!e) return null; \
                     try {{ e.scrollIntoView({{block: 'center'}}); }} catch (_) {{ e.scrollIntoView(); }} const r = e.getBoundingClientRect(); \
                     return [r.x + r.width / 2, r.y + r.height / 2]; }})()",
                    quote(&sel)
                ),
                tap: true,
            }),
            Action::Type { text, .. } => {
                for c in expand(&text, env)?.chars() {
                    key(cx, Key::Character(c.to_string()));
                }
                Some(Wait::Until(Instant::now() + INPUT_SETTLE))
            }
            Action::Key { key: name } => {
                let k = match name.parse::<NamedKey>() {
                    Ok(named) => Key::Named(named),
                    Err(_) if name.chars().count() == 1 => Key::Character(name),
                    Err(_) => return Err(format!("unknown key {name}")),
                };
                key(cx, k);
                Some(Wait::Until(Instant::now() + INPUT_SETTLE))
            }
            Action::Scroll { dy, step } => Some(scroll_chunk(cx, dy, step)),
            Action::Shot { name } => {
                let image = screenshot(cx).ok_or("no image to read")?;
                out.png(&self.suffixed(&name), &image)?;
                None
            }
            Action::Sleep { ms } => Some(Wait::Until(Instant::now() + Duration::from_millis(ms))),
            Action::Mem { name } => {
                let (tx, rx) = mpsc::channel();
                let callback = GenericCallback::new(move |report| {
                    if let Ok(report) = report {
                        let _ = tx.send(report);
                    }
                })
                .map_err(|e| format!("memory report channel: {e:?}"))?;
                cx.servo.create_memory_report(callback);
                Some(Wait::Mem(rx, self.suffixed(&name)))
            }
        })
    }

    /// Check a pending wait; `Ok(None)` means the step is complete.
    fn check(
        &mut self,
        cx: &Cx,
        out: &Out,
        now: Instant,
        wait: Wait,
    ) -> Result<Option<Wait>, String> {
        match wait {
            Wait::Load => {
                Ok((cx.state.loads_complete.get() <= self.nav_mark).then_some(Wait::Load))
            }
            Wait::Until(t) => Ok((now < t).then_some(Wait::Until(t))),
            Wait::Quiet {
                frames,
                since,
                chunk_start,
                left,
                chunk,
                painted_at_start,
            } => {
                let painted = cx.state.frames.get();
                let age = now.duration_since(chunk_start);
                // Pace scroll input to rendering: never send the next chunk before a frame of this one is painted
                // (input faster than frames makes WebRender's work pile up). A page that keeps animating never goes
                // quiet, so a painted chunk also ends at SCROLL_MAX.
                let chunk_done = (painted > painted_at_start
                    && (age >= SCROLL_MAX
                        || (painted == frames && now.duration_since(since) >= SCROLL_QUIET)))
                    || age >= SCROLL_NO_FRAME;
                if chunk_done {
                    Ok((left > 0.0).then(|| scroll_chunk(cx, left, Some(chunk))))
                } else if painted != frames {
                    Ok(Some(Wait::Quiet {
                        frames: painted,
                        since: now,
                        chunk_start,
                        left,
                        chunk,
                        painted_at_start,
                    }))
                } else {
                    Ok(Some(Wait::Quiet {
                        frames,
                        since,
                        chunk_start,
                        left,
                        chunk,
                        painted_at_start,
                    }))
                }
            }
            Wait::Mem(rx, name) => match rx.try_recv() {
                Ok(report) => {
                    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
                    std::fs::write(out.path(&format!("{name}.mem.json")), json)
                        .map_err(|e| e.to_string())?;
                    Ok(None)
                }
                Err(mpsc::TryRecvError::Empty) => Ok(Some(Wait::Mem(rx, name))),
                Err(mpsc::TryRecvError::Disconnected) => Err("memory report failed".into()),
            },
            Wait::Js { expr, tap } => {
                if let Some(result) = self.js.borrow_mut().take() {
                    self.js_inflight = false;
                    match (tap, result) {
                        (_, Err(e)) => return Err(format!("JavaScript: {e}")),
                        (false, Ok(JSValue::Boolean(true))) => return Ok(None),
                        (true, Ok(JSValue::Array(xy))) => {
                            if let [JSValue::Number(x), JSValue::Number(y)] = xy[..] {
                                touch_tap(cx, x as f32, y as f32);
                                return Ok(Some(Wait::Until(now + INPUT_SETTLE)));
                            }
                        }
                        _ => {}
                    }
                }
                if !self.js_inflight && now >= self.next_js {
                    self.js_inflight = true;
                    self.next_js = now + JS_POLL;
                    let slot = self.js.clone();
                    cx.webview.evaluate_javascript(&expr, move |r| {
                        *slot.borrow_mut() = Some(r.map_err(|e| format!("{e:?}")))
                    });
                }
                Ok(Some(Wait::Js { expr, tap }))
            }
        }
    }

    fn finish(&mut self, cx: &Cx, out: &mut Out, ok: bool, note: &str) -> Poll {
        let step = &self.wf.steps[self.idx];
        let (kind, arg) = step.label();
        out.row(&Row {
            step: self.idx,
            kind,
            arg: &arg,
            t_start_ms: self.step_t0,
            t_end_ms: cx.state.ms(),
            ok,
            frames: cx.state.frames.get() - self.frames0,
            max_frame_gap_ms: cx.state.max_gap_ms.get(),
            note,
        });
        self.wait = None;
        self.js_inflight = false;
        self.js.borrow_mut().take();
        self.idx += 1;
        if !ok && !step.optional {
            if let Some(image) = screenshot(cx) {
                let _ = out.png(&self.suffixed(&format!("fail-{}", self.idx - 1)), &image);
            }
            self.failed = true;
            self.idx = self.wf.steps.len();
            self.soak_until = None;
        }
        Poll::Pending
    }

    fn navigate(&mut self, cx: &Cx, url: &str) -> Result<(), String> {
        let url = url::Url::parse(url).map_err(|e| format!("{url}: {e}"))?;
        self.nav_mark = cx.state.loads_complete.get();
        cx.webview.load(url);
        Ok(())
    }

    /// Soak: start the workflow again from its first page.
    fn restart(&mut self, cx: &Cx) {
        self.iteration += 1;
        self.idx = 0;
        let url = self.wf.url.clone();
        if let Err(e) = self.navigate(cx, &url) {
            eprintln!("soak restart: {e}");
        }
    }

    /// Names of shots and reports get the soak iteration so repeats don't overwrite each other.
    fn suffixed(&self, name: &str) -> String {
        if self.iteration == 0 {
            name.to_string()
        } else {
            format!("{name}.{}", self.iteration)
        }
    }
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).expect("string to JSON")
}

/// A fresh frame, read back from the rendering context (not `take_screenshot`, which waits for page readiness).
pub fn screenshot(cx: &Cx) -> Option<servo::RgbaImage> {
    cx.webview.paint();
    let size = cx.ctx.size2d().to_i32();
    cx.ctx.read_to_image(DeviceIntRect::from_size(size))
}

/// Send one wheel scroll of at most `step` (default 300) CSS pixels of `dy`; the rest is left for the next chunk.
fn scroll_chunk(cx: &Cx, dy: f32, step: Option<f32>) -> Wait {
    let chunk = step.unwrap_or(300.0).max(1.0);
    let now_dy = dy.signum() * dy.abs().min(chunk);
    let size = cx.webview.size();
    let dpr = cx.webview.hidpi_scale_factor().get();
    let centre = WebViewPoint::Page(Point2D::new(
        size.width / dpr / 2.0,
        size.height / dpr / 2.0,
    ));
    // WheelDelta.y > 0 scrolls up, so scrolling down is negative; the delta is in device pixels.
    let delta = WheelDelta {
        x: 0.0,
        y: -(now_dy as f64) * dpr as f64,
        z: 0.0,
        mode: WheelMode::DeltaPixel,
    };
    cx.webview
        .notify_input_event(InputEvent::Wheel(WheelEvent::new(delta, centre)));
    let now = Instant::now();
    Wait::Quiet {
        frames: cx.state.frames.get(),
        since: now,
        chunk_start: now,
        left: dy - now_dy,
        chunk,
        painted_at_start: cx.state.frames.get(),
    }
}

fn key(cx: &Cx, key: Key) {
    for state in [KeyState::Down, KeyState::Up] {
        let event = KeyboardEvent::new_without_event(
            state,
            key.clone(),
            Code::Unidentified,
            Location::Standard,
            Modifiers::empty(),
            false,
            false,
        );
        cx.webview.notify_input_event(InputEvent::Keyboard(event));
    }
}

/// A touch down/up at a CSS-pixel point; Servo turns it into a click.
fn touch_tap(cx: &Cx, x: f32, y: f32) {
    for kind in [TouchEventType::Down, TouchEventType::Up] {
        let point = WebViewPoint::Page(Point2D::new(x, y));
        let event = TouchEvent::new(kind, TouchId(1), point, TouchPointerType::Touch);
        cx.webview.notify_input_event(InputEvent::Touch(event));
    }
}
