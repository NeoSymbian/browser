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
//! - **Controls (WB5a):** keys become Servo keyboard presses; a focused field's input method, script dialogs, page
//!   unloads, `<select>` and file pickers go to NEOWEB as events with an id and wait in [`Controls`] for the answer.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dpi::PhysicalSize;
use e32::chunk::Chunk;
use e32::server::{Message, Server, Service};
use e32::sys::TVersion;
use euclid::Scale;
use neoweb_proto::{
    Command, DialogKind, Event, FrameHeader, Load, PressKind, Rect, SelectItem, SelectOp, TouchKind,
    func,
};
use servo::{
    AllowOrDenyRequest, ClipboardDelegate, MouseButton, MouseButtonAction, MouseButtonEvent,
    MouseMoveEvent, Code, ConsoleLogLevel, CreateNewWebViewRequest, JSValue,
    PrefValue, StorageType, DevicePoint, EmbedderControl, EmbedderControlId,
    EventLoopWaker, FilePicker, ImeEvent, InputEvent, InputMethodType, Key, KeyState,
    KeyboardEvent, LoadStatus, Location, Modifiers, NamedKey, RenderingContext, SelectElement,
    SelectElementOption, SelectElementOptionOrOptgroup, SimpleDialog, StringRequest, TouchEvent, TouchEventType, TouchId,
    TouchPointerType, UserContentManager, UserScript, WebView, WebViewBuilder, WebViewDelegate,
    WebViewPoint, WheelDelta, WheelEvent, WheelMode,
};

use crate::Waker;
use crate::tabs::Tabs;

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
    controls: RefCell<Controls>,
    tabs: RefCell<Tabs>,
    downloads: RefCell<crate::downloads::Downloads>,
    /// for a page's new windows
    ctx: Rc<crate::swgl::SwglRenderingContext>,
    dpr: std::cell::Cell<f32>,
}

impl Delegate {
    fn post(&self, e: Event) {
        self.shared.lock().unwrap().post(e);
    }
}

/// A control NEOWEB shows for the page, until its answer comes.
enum Pending {
    Dialog(SimpleDialog),
    Unload(AllowOrDenyRequest),
    Select(SelectElement),
    Files(FilePicker),
    Clipboard(StringRequest),
}

/// The controls waiting for NEOWEB, by protocol id. An unanswered one is answered by its `Drop` (Servo's default:
/// dismissed) when it is hidden or the engine ends.
#[derive(Default)]
struct Controls {
    next: u32,
    open: Vec<(u32, Option<EmbedderControlId>, Pending)>,
    /// The focused field's input method while it has the keyboard.
    ime: Option<EmbedderControlId>,
    /// Ctrl (0x11) came from the keyboard's edit bar: the next key goes with it.
    ctrl: bool,
}

impl Controls {
    fn add(&mut self, control: Option<EmbedderControlId>, p: Pending) -> u32 {
        self.next = self.next.wrapping_add(1);
        self.open.push((self.next, control, p));
        self.next
    }

    fn take(&mut self, id: u32) -> Option<Pending> {
        let i = self.open.iter().position(|(i, ..)| *i == id)?;
        Some(self.open.remove(i).2)
    }
}

/// The start of the console line helper.js writes for a download link.
const DOWNLOAD_MARK: &str = "neoweb:download ";

/// The start of the console line [`LOGIN_SCRIPT`] writes.
const LOGIN_MARK: &str = "neoweb:login ";

/// Run in every page: a form sent with a password reports its user name, for Save password (br2-login-save, B11).
const LOGIN_SCRIPT: &str = r#"addEventListener('submit', e => {
  const f = e.target, pw = f.querySelector && f.querySelector('input[type=password]');
  if (!pw || !pw.value || e.defaultPrevented) return;
  const u = [...f.querySelectorAll('input')].find(i => i !== pw && /^(text|email|tel|)$/.test(i.type) && i.value);
  console.log('neoweb:login ' + (u ? u.value : ''));
}, true);"#;

/// The neoui keyboard (`KEYBOARD` mode) for a field: number and phone pads, else text.
fn keyboard_mode(t: InputMethodType) -> u8 {
    match t {
        InputMethodType::Number => 3,
        InputMethodType::Tel => 4,
        _ => 1,
    }
}

impl Delegate {
    /// A thread-safe way to post events (downloads run on their own threads).
    fn poster(&self) -> crate::downloads::Post {
        let shared = self.shared.clone();
        Arc::new(move |e| shared.lock().unwrap().post(e))
    }

    /// A page event of `view`: the shown tab's as is, another tab's as TabInfo (WB5c).
    fn page(&self, view: &WebView, e: Event) {
        let routed = self.tabs.borrow_mut().route(view, e);
        if let Some(e) = routed {
            self.post(e);
        }
    }

    fn shown(&self, view: &WebView) -> bool {
        let tabs = self.tabs.borrow();
        tabs.shown_view().is_some_and(|v| v.id() == view.id())
    }
}

impl WebViewDelegate for Delegate {
    fn notify_load_status_changed(&self, view: WebView, status: LoadStatus) {
        let load = match status {
            LoadStatus::Started => Load::Started,
            LoadStatus::HeadParsed => Load::HeadParsed,
            LoadStatus::Complete | LoadStatus::Stopped => Load::Complete,
        };
        self.page(&view, Event::Load(load));
    }

    fn notify_url_changed(&self, view: WebView, url: url::Url) {
        self.page(&view, Event::Url(url.into()));
    }

    fn notify_page_title_changed(&self, view: WebView, title: Option<String>) {
        self.page(&view, Event::Title(title.unwrap_or_default()));
    }

    fn notify_history_changed(&self, view: WebView, entries: Vec<url::Url>, current: usize) {
        let e = Event::History {
            back: current > 0,
            forward: current + 1 < entries.len(),
        };
        self.page(&view, e);
    }

    fn notify_new_frame_ready(&self, view: WebView) {
        if self.shown(&view) {
            self.frame_ready.set(true);
        }
    }

    /// A page's new window (`target=_blank`, `window.open`): a tab of its own, shown at once (WB5c).
    fn request_create_new(&self, parent: WebView, request: CreateNewWebViewRequest) {
        let mut b = request
            .builder(self.ctx.clone())
            .hidpi_scale_factor(Scale::new(self.dpr.get()))
            .delegate(parent.delegate())
            .clipboard_delegate(parent.clipboard_delegate());
        if let Some(ucm) = parent.user_content_manager() {
            b = b.user_content_manager(ucm);
        }
        let view = b.build();
        let mut tabs = self.tabs.borrow_mut();
        let opener = tabs.of(&parent).map_or(0, |t| t.id);
        let tab = tabs.engine_id();
        tabs.add(tab, view);
        let events = tabs.show(tab);
        drop(tabs);
        self.post(Event::Created { tab, opener });
        for e in events {
            self.post(e);
        }
        self.frame_ready.set(true);
    }

    fn notify_crashed(&self, _: WebView, reason: String, _backtrace: Option<String>) {
        println!("neowebe: crashed: {reason}");
        self.post(Event::Crashed(reason));
    }

    fn show_embedder_control(&self, _: WebView, control: EmbedderControl) {
        let mut c = self.controls.borrow_mut();
        let ctl = control.id();
        let event = match control {
            EmbedderControl::InputMethod(im) => {
                // a field focused by script, before the user touched the page, gets no keyboard
                if !im.allow_virtual_keyboard() {
                    return;
                }
                c.ime = Some(ctl);
                Event::Keyboard {
                    mode: keyboard_mode(im.input_method_type()),
                }
            }
            EmbedderControl::SimpleDialog(d) => {
                let (kind, text, value) = match &d {
                    SimpleDialog::Alert(a) => (DialogKind::Alert, a.message().to_owned(), String::new()),
                    SimpleDialog::Confirm(a) => (DialogKind::Confirm, a.message().to_owned(), String::new()),
                    SimpleDialog::Prompt(p) => (
                        DialogKind::Prompt,
                        p.message().to_owned(),
                        p.current_value().to_owned(),
                    ),
                };
                let id = c.add(Some(ctl), Pending::Dialog(d));
                Event::Dialog {
                    id,
                    kind,
                    text,
                    value,
                }
            }
            EmbedderControl::SelectElement(sel) => {
                let chosen = sel.selected_options();
                let item = |o: &SelectElementOption| SelectItem {
                    id: o.id as u32,
                    label: o.label.clone(),
                    disabled: o.is_disabled,
                    selected: chosen.contains(&o.id),
                };
                let mut items = Vec::new();
                for o in sel.options() {
                    match o {
                        SelectElementOptionOrOptgroup::Option(o) => items.push(item(o)),
                        SelectElementOptionOrOptgroup::Optgroup { label, options } => {
                            items.push(SelectItem {
                                id: SelectItem::GROUP,
                                label: label.clone(),
                                disabled: true,
                                selected: false,
                            });
                            items.extend(options.iter().map(item));
                        }
                    }
                }
                let multiple = sel.allow_select_multiple();
                let id = c.add(Some(ctl), Pending::Select(sel));
                Event::Select {
                    id,
                    multiple,
                    items,
                }
            }
            EmbedderControl::FilePicker(f) => {
                let accept = f
                    .filter_patterns()
                    .iter()
                    .map(|p| p.0.trim_start_matches('.'))
                    .collect::<Vec<_>>()
                    .join(",");
                let multiple = f.allow_select_multiple();
                let id = c.add(Some(ctl), Pending::Files(f));
                Event::FilePick {
                    id,
                    multiple,
                    accept,
                }
            }
            // the link menu is WB5c; a colour picker keeps its colour (both answer on drop)
            EmbedderControl::ContextMenu(_) | EmbedderControl::ColorPicker(_) => return,
        };
        drop(c);
        self.post(event);
    }

    fn hide_embedder_control(&self, _: WebView, id: EmbedderControlId) {
        let mut c = self.controls.borrow_mut();
        // dropping it answers with Servo's default; a late hide can't close a newer field's keyboard (ids differ)
        c.open.retain(|(_, ctl, _)| *ctl != Some(id));
        if c.ime == Some(id) {
            c.ime = None;
            drop(c);
            self.post(Event::Keyboard { mode: 0 });
        }
    }

    fn show_console_message(&self, _: WebView, _: ConsoleLogLevel, message: String) {
        // LOGIN_SCRIPT's report; a page can fake it, but all it gets is our own "Save password?" sheet
        if let Some(user) = message.strip_prefix(LOGIN_MARK) {
            self.post(Event::Login { user: user.into() });
        } else if let Some(url) = message.strip_prefix(DOWNLOAD_MARK)
            && url.starts_with("http")
        {
            self.downloads.borrow_mut().start(url, self.poster());
        }
    }

    fn request_unload(&self, _: WebView, request: AllowOrDenyRequest) {
        let id = self.controls.borrow_mut().add(None, Pending::Unload(request));
        self.post(Event::Dialog {
            id,
            kind: DialogKind::Unload,
            text: "Leave this page? Changes you made may not be saved.".into(),
            value: String::new(),
        });
    }
}

/// The page's clipboard is the shell's (through NEOWEB).
impl ClipboardDelegate for Delegate {
    fn get_text(&self, _: WebView, request: StringRequest) {
        let id = self
            .controls
            .borrow_mut()
            .add(None, Pending::Clipboard(request));
        self.post(Event::ClipboardGet { id });
    }

    fn set_text(&self, _: WebView, text: String) {
        self.post(Event::ClipboardSet(text));
    }
}

/// NEOWEB's answer to control `id` (an unknown or late id is ignored: its control was hidden).
fn answer(d: &Delegate, cmd: Command) {
    let id = match &cmd {
        Command::Answer { id, .. }
        | Command::Choose { id, .. }
        | Command::Files { id, .. }
        | Command::Clipboard { id, .. } => *id,
        _ => return,
    };
    let Some(p) = d.controls.borrow_mut().take(id) else {
        return;
    };
    match (p, cmd) {
        (Pending::Dialog(SimpleDialog::Prompt(mut p)), Command::Answer { ok: true, text, .. }) => {
            p.set_current_value(&text);
            p.confirm();
        }
        (Pending::Dialog(dlg), Command::Answer { ok, .. }) => {
            if ok {
                dlg.confirm()
            } else {
                dlg.dismiss()
            }
        }
        (Pending::Unload(r), Command::Answer { ok, .. }) => {
            if ok {
                r.allow()
            } else {
                r.deny()
            }
        }
        (Pending::Select(mut sel), Command::Choose { cancel, picked, .. }) => {
            if !cancel {
                sel.select(picked.into_iter().map(|i| i as usize).collect());
                sel.submit();
            }
        }
        (Pending::Files(mut f), Command::Files { paths, .. }) => {
            if paths.is_empty() {
                f.dismiss();
            } else {
                f.select(&paths.into_iter().map(Into::into).collect::<Vec<_>>());
                f.submit();
            }
        }
        (Pending::Clipboard(r), Command::Clipboard { text, .. }) => r.success(text),
        _ => {} // a mismatched answer: the control answers on drop
    }
}

/// A key from the shell's keyboard (presses only) as a Servo press and release.
fn key(webview: &WebView, controls: &RefCell<Controls>, code: u32) {
    let mut c = controls.borrow_mut();
    if code == 0x11 {
        c.ctrl = true;
        return;
    }
    let modifiers = if std::mem::take(&mut c.ctrl) {
        Modifiers::CONTROL
    } else {
        Modifiers::empty()
    };
    drop(c);
    let key = match code {
        8 => Key::Named(NamedKey::Backspace),
        9 => Key::Named(NamedKey::Tab),
        13 => Key::Named(NamedKey::Enter),
        27 => Key::Named(NamedKey::Escape),
        0xF807 => Key::Named(NamedKey::ArrowLeft),
        0xF808 => Key::Named(NamedKey::ArrowRight),
        0xF809 => Key::Named(NamedKey::ArrowUp),
        0xF80A => Key::Named(NamedKey::ArrowDown),
        _ => match char::from_u32(code) {
            Some(ch) if !ch.is_control() => Key::Character(ch.to_string()),
            _ => return,
        },
    };
    for state in [KeyState::Down, KeyState::Up] {
        let event = KeyboardEvent::new_without_event(
            state,
            key.clone(),
            Code::Unidentified,
            Location::Standard,
            modifiers,
            false,
            false,
        );
        webview.notify_input_event(InputEvent::Keyboard(event));
    }
}

/// Frame timing on the debug log, every 2 s while frames are made.
#[derive(Default)]
struct Stats {
    frames: u32,
    paint: Duration,
    copy: Duration,
    since: Option<std::time::Instant>,
}

impl Stats {
    fn add(&mut self, paint: Duration, copy: Duration) {
        self.since.get_or_insert_with(std::time::Instant::now);
        self.frames += 1;
        self.paint += paint;
        self.copy += copy;
    }

    fn report(&mut self) {
        if let Some(t) = self.since
            && t.elapsed() >= Duration::from_secs(2)
        {
            let n = self.frames.max(1);
            println!(
                "neowebe: {} frames/{} ms: paint avg {} ms, copy avg {} ms",
                self.frames,
                t.elapsed().as_millis(),
                self.paint.as_millis() / n as u128,
                self.copy.as_millis() / n as u128
            );
            *self = Self::default();
        }
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

fn input(webview: &WebView, d: &Delegate, cmd: Command) {
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
        Command::Key { code, down: true } => key(webview, &d.controls, code),
        Command::Text(t) => t.chars().for_each(|ch| key(webview, &d.controls, ch.into())),
        Command::ImeDismissed => {
            d.controls.borrow_mut().ime = None;
            webview.notify_input_event(InputEvent::Ime(ImeEvent::Dismissed));
        }
        other => answer(d, other),
    }
}

/// Run the engine server until the last client goes. Returns the exit reason.
/// The calling thread's death ends the process (`User::SetCritical(EProcessCritical)`): if Servo's main thread dies,
/// the IPC thread would otherwise keep the server alive and NEOWEB would wait on a page that never comes.
fn process_critical() {
    const CURRENT_THREAD: u32 = 0xFFFF_8001; // KCurrentThreadHandle
    const CRITICAL_FLAGS: u32 = 0xF; // system/process permanent/critical (u32std.h)
    const PROCESS_CRITICAL: u32 = 0x1; // KThreadFlagProcessCritical
    // SAFETY: ThreadSetFlags(handle, clear, set) on the current thread, as euser's User::SetCritical calls it
    unsafe {
        e32::sys::swi::exec4::<{ e32::sys::execs::slow::THREAD_SET_FLAGS }>(
            CURRENT_THREAD,
            CRITICAL_FLAGS,
            PROCESS_CRITICAL,
            0,
        );
    }
}

pub fn serve(log: log::LevelFilter) -> Result<(), String> {
    process_critical();
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
        controls: Default::default(),
        tabs: Default::default(),
        downloads: Default::default(),
        ctx: ctx.clone(),
        dpr: std::cell::Cell::new(2.75),
    });
    let user_content = Rc::new(UserContentManager::new(&servo));
    user_content.add_script(Rc::new(UserScript::new(LOGIN_SCRIPT.into(), None)));
    user_content.add_script(Rc::new(UserScript::new(HELPER_SCRIPT.into(), None)));
    let mobile_agent = servo::pref!(user_agent);
    // Settings' text size, for every page (new ones too)
    let zoom = std::cell::Cell::new(1.0f32);
    // After a resize while a field has the keyboard, the field is scrolled into view once the page has the new size.
    let mut reveal_at: Option<std::time::Instant> = None;
    let mut buffers = Buffers::default();
    let mut pending = false;
    let mut stats = Stats::default();
    // Debug aid: with the file C:/neowebe.check present, the whole heap is checked after every step (a corrupt
    // heap panics there, just after the line naming the step) and every 500 ms when idle.
    let check_heap = std::path::Path::new("C:/neowebe.check").exists();
    let mut last_check = std::time::Instant::now();
    let check = |what: &dyn std::fmt::Display| {
        if check_heap {
            println!("neowebe: hc {what}");
            unsafe { crate::cxxrt_heap_check() };
        }
    };
    if check_heap {
        println!("neowebe: heap checks on");
    }
    // Show tab `tab`, made with `url` if it has no page (a WebView is made with its URL: a load sent before the
    // constellation has made its browsing context is dropped).
    let open = |tab: u32, url: Option<url::Url>, size: PhysicalSize<u32>| {
        let exists = delegate.tabs.borrow().get(tab).is_some();
        match (exists, url) {
            (true, Some(u)) => delegate.tabs.borrow().get(tab).unwrap().view.load(u),
            (true, None) => {}
            (false, Some(u)) => {
                ctx.resize(size);
                let view = WebViewBuilder::new(&servo, ctx.clone())
                    .url(u)
                    .hidpi_scale_factor(Scale::new(delegate.dpr.get()))
                    .delegate(delegate.clone())
                    .clipboard_delegate(delegate.clone())
                    .user_content_manager(user_content.clone())
                    .build();
                view.set_page_zoom(zoom.get());
                delegate.tabs.borrow_mut().add(tab, view);
            }
            (false, None) => return,
        }
        let events = delegate.tabs.borrow_mut().show(tab);
        for e in events {
            delegate.post(e);
        }
    };
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
            let name = format!("{cmd:?}");
            // never slice inside a character (a command can carry any text)
            check(&format_args!("before {}", &name[..name.floor_char_boundary(80)]));
            let shown = delegate.tabs.borrow().shown;
            match cmd {
                // 1.1's single page: tab 0 (a 1.2 NEOWEB opens tabs)
                Command::Load(u) | Command::Open { url: u, .. } if url::Url::parse(&u).is_err() => {
                    delegate.post(Event::Crashed(format!("{u}: not an address")));
                }
                Command::Load(u) => open(shown.unwrap_or(0), url::Url::parse(&u).ok(), size),
                Command::Open { tab, url } => {
                    open(tab, url::Url::parse(&url).ok(), size);
                    pending = true;
                }
                Command::Activate { tab } => {
                    open(tab, None, size);
                    pending = true;
                }
                Command::Close { tab } => delegate.tabs.borrow_mut().close(tab),
                Command::Download { url } => {
                    delegate.downloads.borrow_mut().start(&url, delegate.poster())
                }
                Command::Zoom(p) => {
                    zoom.set(p.clamp(50, 300) as f32 / 100.0);
                    for t in &delegate.tabs.borrow().list {
                        t.view.set_page_zoom(zoom.get());
                    }
                }
                Command::ClearData => {
                    servo.site_data_manager().clear_cookies(None);
                    servo
                        .site_data_manager()
                        .clear_site_data(&[], StorageType::all());
                    servo.network_manager().clear_cache();
                }
                Command::DownloadOp { id, op } => {
                    delegate.downloads.borrow_mut().op(id, op, delegate.poster())
                }
                Command::DesktopSite(on) => {
                    let agent = if on { DESKTOP_AGENT.into() } else { mobile_agent.clone() };
                    servo.set_preference("user_agent", PrefValue::Str(agent));
                }
                Command::Resize { w, h, dpr_milli } => {
                    let new = PhysicalSize::new(w.clamp(1, MAX_W), h.clamp(1, MAX_H));
                    if dpr_milli > 0 {
                        delegate.dpr.set(dpr_milli as f32 / 1000.0);
                    }
                    // the rendering context is shared: every page gets the size
                    for t in &delegate.tabs.borrow().list {
                        if new != size {
                            t.view.resize(new);
                        }
                        t.view.set_hidpi_scale_factor(Scale::new(delegate.dpr.get()));
                    }
                    println!("neowebe: resize {}x{} dpr {}", new.width, new.height, delegate.dpr.get());
                    if new != size && delegate.controls.borrow().ime.is_some() {
                        // shortcut: a fixed delay for the page's relayout; an engine hook if fields land off screen
                        reveal_at = Some(std::time::Instant::now() + Duration::from_millis(150));
                    }
                    size = new;
                    pending = true;
                }
                Command::FrameShown { seq } => buffers.shown(seq),
                // the page's own controls, answered whichever tab is shown
                c @ (Command::Answer { .. }
                | Command::Choose { .. }
                | Command::Files { .. }
                | Command::Clipboard { .. }) => answer(&delegate, c),
                cmd => {
                    let tabs = delegate.tabs.borrow();
                    let Some(view) = tabs.shown_view().cloned() else {
                        continue; // nothing to act on before the first page
                    };
                    drop(tabs);
                    match cmd {
                        Command::Reload => view.reload(),
                        Command::Stop => view.evaluate_javascript("window.stop()", |_| {}),
                        Command::Back => {
                            view.go_back(1);
                        }
                        Command::Forward => {
                            view.go_forward(1);
                        }
                        Command::Press { x, y } => press(&view, &shared, x, y),
                        Command::Select { op, x, y } => select(&view, &shared, op, x, y),
                        Command::Find { text, dir } => find(&view, &shared, &text, dir),
                        other => input(&view, &delegate, other),
                    }
                }
            }
        }
        if let Some(t) = reveal_at
            && std::time::Instant::now() >= t
        {
            reveal_at = None;
            if let Some(w) = delegate.tabs.borrow().shown_view() {
                w.evaluate_javascript(
                    "document.activeElement && document.activeElement.scrollIntoView({block:'nearest'})",
                    |_| {},
                );
            }
        }
        servo.spin_event_loop();
        run_deferred();
        if check_heap && last_check.elapsed() >= Duration::from_millis(500) {
            last_check = std::time::Instant::now();
            unsafe { crate::cxxrt_heap_check() };
        }
        if delegate.frame_ready.replace(false) {
            pending = true;
        }
        let view = delegate.tabs.borrow().shown_view().cloned();
        if pending
            && let Some(webview) = &view
            && let Some(i) = buffers.free()
        {
            pending = false;
            check(&"before paint");
            let t0 = std::time::Instant::now();
            webview.paint();
            let t1 = std::time::Instant::now();
            check(&"after paint");
            // SAFETY: buffer i is inside the chunk (FrameHeader::layout), MAX_H rows of `stride` bytes, not held
            let dst = unsafe { chunk.base().add(header.offsets[i] as usize) };
            if let Some((w, h)) = ctx.copy_frame(dst, header.stride as usize, MAX_W, MAX_H) {
                stats.add(t1 - t0, t1.elapsed());
                check(&"after copy");
                buffers.seq = buffers.seq.wrapping_add(1);
                buffers.held[i] = Some(buffers.seq);
                delegate.post(Event::Frame {
                    seq: buffers.seq,
                    buffer: i as u8,
                    width: w,
                    height: h,
                    dirty: Rect {
                        x: 0,
                        y: 0,
                        w: w as i32,
                        h: h as i32,
                    },
                });
            }
        }
        stats.report();
        waker.wait(Duration::from_millis(20));
    }
    println!("neowebe: last client gone, exiting");
    Ok(())
}

/// The page helper (helper.js): long-press, selection, find (WB5c).
const HELPER_SCRIPT: &str = include_str!("helper.js");

/// The user agent for Desktop site: a desktop Firefox's form, with Servo named.
const DESKTOP_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Servo/0.8.0 Firefox/140.0";

/// `s` as a JavaScript string literal.
fn js_str(s: &str) -> String {
    let mut o = String::from('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                o.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// A helper result as numbers and strings.
fn items(v: &Result<JSValue, servo::JavaScriptEvaluationError>) -> Vec<JSValue> {
    match v {
        Ok(JSValue::Array(a)) => a.clone(),
        Ok(other) => vec![other.clone()],
        Err(e) => {
            println!("neowebe: helper failed: {e:?}");
            Vec::new()
        }
    }
}

fn num(v: Option<&JSValue>) -> i32 {
    match v {
        Some(JSValue::Number(n)) => n.round() as i32,
        _ => 0,
    }
}

fn text(v: Option<&JSValue>) -> String {
    match v {
        Some(JSValue::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn post(shared: &SharedRef, e: Event) {
    shared.lock().unwrap().post(e);
}


/// The text selection's ends as points (page px): Servo 0.8's Range rectangles cover a whole text node (and, in one
/// node, everything after it), so the engine keeps the points it selected with and reports those.
#[derive(Clone, Copy, Default)]
pub struct SelEnds {
    a: (i32, i32),
    b: (i32, i32),
    line: i32,
}

thread_local!(static SEL: std::cell::Cell<Option<SelEnds>> = const { std::cell::Cell::new(None) });

type Deferred = Box<dyn FnOnce()>;

// Work a script result asks for (more input, more script): Servo runs those callbacks while its evaluator is
// borrowed, so calling back into the WebView from one panics ("RefCell already borrowed"). The main loop runs them
// after spin_event_loop.
thread_local!(static DEFER: RefCell<Vec<Deferred>> = RefCell::new(Vec::new()));

fn defer(f: impl FnOnce() + 'static) {
    DEFER.with_borrow_mut(|d| d.push(Box::new(f)));
}

fn run_deferred() {
    for f in DEFER.with_borrow_mut(std::mem::take) {
        f();
    }
}

fn sel_event(e: Option<SelEnds>) -> Event {
    Event::Selection {
        ends: e.map_or(Vec::new(), |e| {
            vec![e.a.0, e.a.1 - e.line / 2, e.b.0, e.b.1 - e.line / 2, e.line]
        }),
    }
}

fn mouse(view: &WebView, action: Option<MouseButtonAction>, x: i32, y: i32) {
    let point = WebViewPoint::Device(DevicePoint::new(x as f32, y as f32));
    let e = match action {
        Some(a) => InputEvent::MouseButton(MouseButtonEvent::new(a, MouseButton::Primary, point)),
        None => InputEvent::MouseMove(MouseMoveEvent::new(point)),
    };
    view.notify_input_event(e);
}

/// Select from `anchor` to `to` the way a mouse drag does (Servo's document selection).
fn drag_select(view: &WebView, anchor: (i32, i32), to: (i32, i32)) {
    mouse(view, Some(MouseButtonAction::Down), anchor.0, anchor.1);
    mouse(view, None, to.0, to.1);
    mouse(view, Some(MouseButtonAction::Up), to.0, to.1);
}

/// A long-press: a link or image for the menu, or the word under it selected (a double click).
fn press(view: &WebView, shared: &SharedRef, x: i32, y: i32) {
    let shared = shared.clone();
    let v2 = view.clone();
    view.evaluate_javascript(&format!("__neoweb.press({x},{y})"), move |r| {
        let v = items(&r);
        let kind = match num(v.first()) {
            1 => PressKind::Link,
            2 => PressKind::Image,
            _ => PressKind::Nothing,
        };
        if kind == PressKind::Nothing {
            defer(move || select_word(&v2, shared, x, y));
            return;
        }

        post(
            &shared,
            Event::Pressed {
                kind,
                title: text(v.get(1)),
                url: text(v.get(2)),
                y: num(v.get(3)),
            },
        );
    });
}

/// Select the word at (x, y) with a double click (Servo's word segmentation), then report the selection.
fn select_word(view: &WebView, shared: SharedRef, x: i32, y: i32) {
    for _ in 0..2 {
        mouse(view, Some(MouseButtonAction::Down), x, y);
        mouse(view, Some(MouseButtonAction::Up), x, y);
    }
    view.evaluate_javascript(
        "[getSelection().toString().length, devicePixelRatio]",
        move |r| {
            let v = items(&r);
            if num(v.first()) == 0 {
                return post(&shared, sel_event(None));
            }
            // shortcut: the word's real ends need text geometry Servo doesn't give script; the handles sit a
            // word's width around the press
            let dpr = num(v.get(1)).max(1);
            let half = 30 * dpr;
            let e = SelEnds {
                a: (x - half, y),
                b: (x + half, y),
                line: 20 * dpr,
            };
            SEL.set(Some(e));
            post(&shared, sel_event(Some(e)));
            post(
                &shared,
                Event::Pressed {
                    kind: PressKind::Text,
                    title: String::new(),
                    url: String::new(),
                    y: 0,
                },
            );
        },
    );
}

fn select(view: &WebView, shared: &SharedRef, op: SelectOp, x: i32, y: i32) {
    let shared = shared.clone();
    if op == SelectOp::Copy {
        view.evaluate_javascript("__neoweb.text()", move |r| {
            post(&shared, Event::ClipboardSet(text(items(&r).first())));
        });
        return;
    }
    match (op, SEL.get()) {
        // a handle dragged: select from the other end to the finger, as a mouse would
        (SelectOp::DragEnd, Some(mut e)) => {
            e.b = (x, y + e.line / 2);
            drag_select(view, e.a, e.b);
            SEL.set(Some(e));
            post(&shared, sel_event(Some(e)));
        }
        (SelectOp::DragStart, Some(mut e)) => {
            e.a = (x, y + e.line / 2);
            drag_select(view, e.b, e.a);
            SEL.set(Some(e));
            post(&shared, sel_event(Some(e)));
        }
        (SelectOp::SelectAll, _) => {
            view.evaluate_javascript(
                "__neoweb.selectAll(); [innerWidth * devicePixelRatio, innerHeight * devicePixelRatio, devicePixelRatio]",
                move |r| {
                    let v = items(&r);
                    let (w, h, dpr) = (num(v.first()), num(v.get(1)), num(v.get(2)).max(1));
                    let e = SelEnds {
                        a: (16 * dpr, 40 * dpr),
                        b: (w - 16 * dpr, h - 60 * dpr),
                        line: 20 * dpr,
                    };
                    SEL.set(Some(e));
                    post(&shared, sel_event(Some(e)));
                },
            );
        }
        _ => {
            SEL.set(None);
            view.evaluate_javascript("__neoweb.clear()", move |_| post(&shared, sel_event(None)));
        }
    }
}

fn find(view: &WebView, shared: &SharedRef, q: &str, dir: i32) {
    let shared = shared.clone();
    view.evaluate_javascript(&format!("__neoweb.find({},{dir})", js_str(q)), move |r| {
        let v = items(&r);
        post(
            &shared,
            Event::Found {
                index: num(v.first()).max(0) as u32,
                count: num(v.get(1)).max(0) as u32,
            },
        );
    });
}
