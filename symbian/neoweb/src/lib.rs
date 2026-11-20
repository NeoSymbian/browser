// SPDX-License-Identifier: GPL-3.0-only
//! NEOWEB.EXE owns the browser chrome and displays frames from NEOWEBE.EXE.
#![no_std]

extern crate alloc;

mod engine;

use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::future::pending;
use core::pin::pin;
use e32::dprintln;
use e32::ipc::TIpcArgs;
use e32::rt::{self, Either};
use e32::sys::TPtr8;
use engine::{Connection, Update};
use neoapp::{App, AppEvent, Page, Query};
use neoui_proto::{Fingers, InputEvent, TouchPhase, ipc};
use neoweb_core::controls::{self, BackStep};
use neoweb_core::input::{self, Suggestion};
use neoweb_core::{Header, LiveTab, START, count_label, start};
use neoweb_proto::{Command, DialogKind, Event, SelectItem, TouchKind};
use neoweb_ui::{
    BrowserApp, BrowserState, KeyboardFocus, Kit, SelectRow, ShortcutTile, Suggest, Theme,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

e32::main!(main);

fn main() -> i32 {
    match run() {
        Ok(()) => 0,
        Err(e) => {
            dprintln!("neoweb: failed {}", e);
            e
        }
    }
}

/// A control of the page waiting for the user's answer (WB5a): what to answer with its id.
#[derive(Default)]
enum Control {
    #[default]
    None,
    Prompt(u32),
    Select {
        id: u32,
        multiple: bool,
        items: Vec<SelectItem>,
    },
    Upload {
        id: u32,
        accept: String,
    },
}

/// One tab for now (Opened pages is UI2), the pages visited (title, address; oldest first), the chrome's state.
#[derive(Default)]
struct Model {
    live: LiveTab,
    showing_web: bool,
    visited: Vec<(String, String)>,
    editing: bool,
    suggestions: Vec<Suggestion>,
    fingers: Fingers,
    /// the keyboard last asked of the UI server: (open, KeyboardFocus.seq, mode)
    keyboard: (bool, i32, i32),
    /// the keyboard the focused page field wants (a neoui KEYBOARD mode, 0 = none; WB5a)
    page_keys: u8,
    /// the open sheet's control, the shell's QUERY's dialog, and a file pick waiting for Files
    control: Control,
    query: Option<u32>,
    picking: Option<u32>,
    /// sites the user said Never to (Save password)
    never_save: Vec<String>,
    /// the page size last sent to the engine
    sent: Option<Command>,
}

impl Model {
    fn url(&self) -> &str {
        if self.showing_web {
            &self.live.url
        } else {
            START
        }
    }

    fn go(&mut self, ui: &BrowserApp, url: String) -> Option<Command> {
        self.end_edit(ui);
        self.showing_web = url != START;
        if !self.showing_web {
            return None;
        }
        self.live.url = url.clone();
        self.live.title.clear();
        self.live.crashed = false;
        Some(Command::Load(url))
    }

    fn end_edit(&mut self, ui: &BrowserApp) {
        self.editing = false;
        ui.global::<KeyboardFocus>().invoke_close();
    }

    fn act(&mut self, ui: &BrowserApp, action: &str, i: i32) -> Option<Command> {
        let s = ui.global::<BrowserState>();
        match action {
            "shortcut" => {
                if let Some(t) = start::SHORTCUTS.get(i as usize) {
                    return self.go(ui, t.url.into());
                }
            }
            "recent" => {
                let urls = self.visited.iter().map(|(_, u)| u.as_str());
                if let Some(r) = start::recent_urls(urls).get(i as usize) {
                    return self.go(ui, String::from(*r));
                }
            }
            "address" | "search" => {
                let url = self.url();
                s.set_edit_text(if url == START { "".into() } else { url.into() });
                self.editing = true;
                self.suggest(&s.get_edit_text());
            }
            "edited" => self.suggest(&s.get_edit_text()),
            "go" => {
                let text = s.get_edit_text();
                if !text.trim().is_empty() {
                    return self.go(ui, input::parse(&text));
                }
            }
            "suggestion" => {
                let url = match self.suggestions.get(i as usize) {
                    Some(Suggestion::History(_, u) | Suggestion::Bookmark(_, u)) => u.clone(),
                    Some(Suggestion::Search(t)) => input::search(t),
                    None => return None,
                };
                return self.go(ui, url);
            }
            "cancel-edit" => self.end_edit(ui),
            "back" => match controls::back_step(self.editing, self.showing_web, self.live.back) {
                BackStep::EndEdit => self.end_edit(ui),
                BackStep::History => return Some(Command::Back),
                BackStep::StartPage | BackStep::Shell => self.showing_web = false,
            },
            "forward" if !self.showing_web => self.showing_web = !self.live.url.is_empty(),
            "forward" if self.live.forward => return Some(Command::Forward),
            "reload" if self.live.crashed => return self.go(ui, self.live.url.clone()),
            "reload" if self.showing_web => return Some(Command::Reload),
            "stop" => {
                self.live.loading.stop();
                return Some(Command::Stop);
            }
            "prompt" => {
                if let Control::Prompt(id) = core::mem::take(&mut self.control) {
                    return Some(Command::Answer {
                        id,
                        ok: i == 1,
                        text: s.get_sheet_value().into(),
                    });
                }
            }
            "select" => {
                if let Control::Select {
                    id,
                    multiple,
                    items,
                } = &mut self.control
                    && let Some(it) = items.get_mut(i as usize)
                {
                    if !*multiple {
                        let picked = alloc::vec![it.id];
                        let id = *id;
                        self.control = Control::None;
                        return Some(Command::Choose {
                            id,
                            cancel: false,
                            picked,
                        });
                    }
                    it.selected = !it.selected;
                    s.set_select_rows(select_rows(items));
                }
            }
            "select-done" | "select-cancel" => {
                if let Control::Select { id, items, .. } = core::mem::take(&mut self.control) {
                    return Some(Command::Choose {
                        id,
                        cancel: action == "select-cancel",
                        picked: items
                            .iter()
                            .filter(|it| it.selected && it.id != SelectItem::GROUP)
                            .map(|it| it.id)
                            .collect(),
                    });
                }
            }
            "login" if i == 0 => {
                let site = Header::of(&self.live.url).domain;
                if !self.never_save.contains(&site) {
                    self.never_save.push(site);
                }
            }
            "upload-cancel" => {
                if let Control::Upload { id, .. } = core::mem::take(&mut self.control) {
                    return Some(Command::Files {
                        id,
                        paths: Vec::new(),
                    });
                }
            }
            // "upload" opens Files (App needed: run's loop); "login" waits for WB6's store
            // tabs, Options, bookmarks, site info, edit shortcuts: UI2–UI5
            _ => {}
        }
        None
    }

    /// The page's controls (WB5a): a field's keyboard, a dialog, a select, a file pick, the clipboard. Returns an
    /// answer to send at once.
    fn control(&mut self, app: &App, ui: &BrowserApp, event: Event) -> Option<Command> {
        let s = ui.global::<BrowserState>();
        let site = Header::of(&self.live.url).domain;
        match event {
            Event::Keyboard { mode } => self.page_keys = mode,
            Event::Dialog {
                id,
                kind: DialogKind::Prompt,
                text,
                value,
            } => {
                s.set_sheet_site(site.as_str().into());
                s.set_sheet_text(text.as_str().into());
                s.set_sheet_value(value.as_str().into());
                self.control = Control::Prompt(id);
                ui.invoke_show_sheet(1);
            }
            Event::Dialog { id, kind, text, .. } => {
                let (text, buttons) = controls::query(&site, kind, &text);
                let q = Query {
                    text,
                    buttons,
                    destructive: false,
                };
                if app.query(&q).is_err() {
                    return Some(Command::Answer {
                        id,
                        ok: false,
                        text: String::new(),
                    });
                }
                // shortcut: one QUERY at a time; a dialog arriving while one is open answers the older as Cancel
                let older = self.query.replace(id);
                return older.map(|id| Command::Answer {
                    id,
                    ok: false,
                    text: String::new(),
                });
            }
            Event::Select {
                id,
                multiple,
                items,
            } => {
                s.set_select_rows(select_rows(&items));
                s.set_select_multiple(multiple);
                self.control = Control::Select {
                    id,
                    multiple,
                    items,
                };
                ui.invoke_show_sheet(2);
            }
            Event::FilePick { id, accept, .. } => {
                // shortcut: no recent files yet (Files keeps no list another app can read)
                s.set_recent_files(model(Vec::new()));
                self.control = Control::Upload { id, accept };
                ui.invoke_show_sheet(3);
            }
            Event::ClipboardSet(text) => {
                // over the shell's 2048 bytes the copy is refused, never cut (browser-ui.md)
                if !app.set_clipboard(&text) {
                    let _ = app.note("Too long to copy", "");
                }
            }
            Event::Login { user } => {
                // B11: no secure store yet, so the sheet only says it; Never is remembered for this run (WB5d keeps it)
                if !self.never_save.contains(&site) {
                    s.set_sheet_site(site.as_str().into());
                    s.set_sheet_text(user.as_str().into());
                    ui.invoke_show_sheet(4);
                }
            }
            Event::ClipboardGet { id } => {
                return Some(Command::Clipboard {
                    id,
                    text: app.clipboard().unwrap_or_default(),
                });
            }
            _ => {}
        }
        None
    }

    /// Open Files to pick the file a page asked for (br2-upload's choice `i`: 0 Photos, 1 Files, 2 Memory card).
    fn pick(&mut self, app: &App, i: i32) -> Option<Command> {
        let Control::Upload { id, accept } = core::mem::take(&mut self.control) else {
            return None;
        };
        let filter = controls::picker_filter(&accept, i == 0);
        let files = neoui_proto::open::files::KEY;
        match app.open(files, neoui_proto::open::files::PICK, &filter) {
            Ok(()) => {
                self.picking = Some(id);
                None
            }
            Err(e) => {
                dprintln!("neoweb: Files picker {}", e);
                Some(Command::Files {
                    id,
                    paths: Vec::new(),
                })
            }
        }
    }

    /// Back from Files: copy the picked file where the engine can read it and answer the file pick.
    fn picked(&mut self, app: &App) -> Option<Command> {
        let id = self.picking.take()?;
        let paths = match app.result_file().map(|f| copy_for_engine(&f)) {
            Some(Ok(path)) => alloc::vec![path],
            Some(Err(e)) => {
                dprintln!("neoweb: upload copy {}", e);
                let _ = app.note("Can't read that file", "");
                Vec::new()
            }
            None => Vec::new(),
        };
        Some(Command::Files { id, paths })
    }

    fn suggest(&mut self, text: &str) {
        self.suggestions = input::suggest(text, &self.visited, &[]);
    }

    fn crash(&mut self, ui: &BrowserApp, reason: String) {
        self.live.apply(Event::Crashed(reason));
        self.fingers.take();
        ui.global::<BrowserState>()
            .set_frame(slint::Image::default());
        self.show(ui);
    }

    fn apply(&mut self, event: Event) {
        let visit = matches!(event, Event::Url(_) | Event::Title(_));
        self.live.apply(event);
        if visit && !self.live.url.is_empty() {
            match self.visited.last_mut() {
                Some((title, url)) if *url == self.live.url => title.clone_from(&self.live.title),
                _ => self
                    .visited
                    .push((self.live.title.clone(), self.live.url.clone())),
            }
        }
    }

    /// Capture a finger at press time so a drag across the toolbar still ends in the page.
    fn touch(&mut self, ui: &BrowserApp, input: InputEvent) -> Option<Command> {
        let InputEvent::Touch { id, x, y, phase } = input else {
            return None;
        };
        let y = y - ui.get_page_top_px();
        let in_page = self.showing_web
            && !self.live.crashed
            && !self.editing
            && (0..ui.get_page_width_px()).contains(&x)
            && (0..ui.get_page_height_px()).contains(&y);
        self.fingers
            .route(id, phase, in_page)
            .then(|| Command::Touch {
                kind: match phase {
                    TouchPhase::Down => TouchKind::Down,
                    TouchPhase::Move => TouchKind::Move,
                    TouchPhase::Up => TouchKind::Up,
                },
                id: id.into(),
                x,
                y,
            })
    }

    /// Put the model into the UI.
    fn show(&self, ui: &BrowserApp) {
        let header = Header::of(self.url());
        let s = ui.global::<BrowserState>();
        s.set_page(if !self.showing_web {
            0
        } else if self.live.crashed {
            5
        } else {
            4
        });
        s.set_security(header.security as i32);
        s.set_domain(header.domain.as_str().into());
        // The local Start page precedes the engine's first history entry.
        s.set_can_back(self.showing_web);
        s.set_can_forward(if self.showing_web {
            self.live.forward
        } else {
            !self.live.url.is_empty()
        });
        s.set_count(count_label(1).as_str().into());
        s.set_loading(self.showing_web && self.live.loading.active());
        s.set_progress(self.live.loading.progress());
        s.set_editing(self.editing);
        // shortcut: the chrome stays shown on live pages (no scroll offset from the engine yet); WB5
        s.set_chrome_shown(true);
        let urls = self.visited.iter().map(|(_, u)| u.as_str());
        let recents: Vec<SharedString> = start::recent(urls)
            .into_iter()
            .map(SharedString::from)
            .collect();
        s.set_recents(model(recents));
        let rows: Vec<Suggest> = self
            .suggestions
            .iter()
            .map(|r| match r {
                Suggestion::History(t, u) => Suggest {
                    kind: 0,
                    title: t.as_str().into(),
                    url: bare(u).into(),
                },
                Suggestion::Bookmark(t, u) => Suggest {
                    kind: 1,
                    title: t.as_str().into(),
                    url: bare(u).into(),
                },
                Suggestion::Search(t) => Suggest {
                    kind: 2,
                    title: t.as_str().into(),
                    url: "".into(),
                },
            })
            .collect();
        s.set_suggestions(model(rows));
    }
}

fn select_rows(items: &[SelectItem]) -> ModelRc<SelectRow> {
    model(
        items
            .iter()
            .map(|it| SelectRow {
                label: it.label.as_str().into(),
                group: it.id == SelectItem::GROUP,
                disabled: it.disabled,
                selected: it.selected,
            })
            .collect(),
    )
}

/// Copy a file Files handed over into the engine's upload folder; its path there, as the engine names it.
fn copy_for_engine(f: &e32::fs::SharedFile) -> Result<String, i32> {
    use e32::fs::{Fs, mode};
    let src = f.file();
    let path = controls::upload_path(&src.full_name()?);
    let fs = Fs::connect()?;
    let sym: String = path.replace('/', "\\");
    let dir = &sym[..=sym.rfind('\\').unwrap_or(0)];
    let _ = fs.mkdir(dir); // it may exist
    let dst = e32::fs::File::replace(
        &fs,
        &sym,
        mode::WRITE | mode::SHARE_EXCLUSIVE | mode::BIG_FILE,
    )?;
    let mut buf = alloc::vec![0u8; 64 * 1024];
    let mut pos = 0u64;
    loop {
        let n = src.read_at64(pos, &mut buf)?;
        if n == 0 {
            break;
        }
        dst.write(&buf[..n])?;
        pos += n as u64;
    }
    dst.flush()?;
    Ok(path)
}

/// Frame timing on the debug log every 2 s while frames come (WB5b): the copy into Slint's image, and the redraw
/// with the shell's DAMAGE (the presentation fence).
#[derive(Default)]
struct FrameStats {
    since: Option<u64>,
    frames: u32,
    copy_us: u64,
    draw_us: u64,
}

impl FrameStats {
    fn add(&mut self, copy_us: u32, draw_us: u32) {
        let now = e32::time::millis();
        let since = *self.since.get_or_insert(now);
        self.frames += 1;
        self.copy_us += u64::from(copy_us);
        self.draw_us += u64::from(draw_us);
        if now - since >= 2000 {
            let n = u64::from(self.frames.max(1));
            dprintln!(
                "neoweb: {} frames/{} ms: copy avg {} ms, draw avg {} ms",
                self.frames,
                now - since,
                self.copy_us / n / 1000,
                self.draw_us / n / 1000
            );
            *self = Self::default();
        }
    }
}

/// Microseconds from `from` (a fast counter value) to now.
fn us_since(from: u32) -> u32 {
    e32::time::counter_us(
        from,
        e32::time::fast_counter(),
        e32::time::fast_counter_hz(),
    )
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

/// An address as the suggestions list shows it: no scheme.
fn bare(url: &str) -> &str {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
}

fn run() -> Result<(), i32> {
    let app = App::connect()?;
    let ui = BrowserApp::new().map_err(|_| -2)?;
    ui.global::<Theme>().set_dp(app.dp);
    let tiles: Vec<ShortcutTile> = start::SHORTCUTS
        .iter()
        .map(|t| ShortcutTile {
            letters: t.letters.into(),
            label: t.label.into(),
            color: slint::Color::from_argb_encoded(0xff00_0000 | t.color),
        })
        .collect();
    ui.global::<BrowserState>().set_shortcuts(model(tiles));
    let mut m = Model::default();
    m.show(&ui);
    let actions = Rc::new(RefCell::new(VecDeque::new()));
    let queue = actions.clone();
    ui.global::<BrowserState>().on_act(move |action, i| {
        queue.borrow_mut().push_back((action, i));
    });
    ui.show().map_err(|_| -2)?;
    let mut shown = Page::default();
    sync(&app, &ui, &mut m, &mut shown)?;
    app.draw()?;

    rt::block_on(async {
        let mut connection = Connection::start();
        let mut stats = FrameStats::default();
        // a frame was installed this turn: its copy time (µs), for the timing after the redraw
        let mut framed: Option<u32> = None;
        let mut next = pin!(shell_event(&app));
        loop {
            // Both NEXT_EVENT requests stay pinned when a timer or the other server wakes us.
            let event = rt::select(next.as_mut(), rt::select(connection.next(), wake(&app))).await;
            let mut result = Ok(());
            match event {
                Either::Left(event) => {
                    let event = event?;
                    next.set(shell_event(&app));
                    if event == AppEvent::Quit {
                        return Ok(());
                    }
                    let page_typing = m.page_keys > 0 && !m.editing && !ui.get_sheet_open();
                    if let AppEvent::Input(input) = event
                        && let Some(command) = m.touch(&ui, input)
                    {
                        result = connection.send(command);
                    } else if let AppEvent::Input(InputEvent::Key { code, down }) = event
                        && page_typing
                    {
                        result = connection.send(Command::Key { code, down });
                    } else if event != AppEvent::Back {
                        app.handle(&event);
                    }
                    match event {
                        // the kit's sheets close first (their cancel comes back as an action)
                        AppEvent::Back if neoapp::kit_back!(ui) => {}
                        AppEvent::Back => actions.borrow_mut().push_back(("back".into(), 0)),
                        AppEvent::Keyboard { h } => {
                            let kf = ui.global::<KeyboardFocus>();
                            kf.set_dismissed(h == 0 && kf.get_open());
                            kf.set_text_h(h as f32);
                            ui.global::<BrowserState>().set_keyboard_px(h as i32);
                            // the user put the page's keyboard away
                            if h == 0 && page_typing {
                                m.page_keys = 0;
                                result = connection.send(Command::ImeDismissed);
                            }
                        }
                        AppEvent::Answer { i } => {
                            if let Some(id) = m.query.take() {
                                result = connection.send(Command::Answer {
                                    id,
                                    ok: i == 0,
                                    text: String::new(),
                                });
                            }
                        }
                        AppEvent::Visible(true) => {
                            if let Some(c) = m.picked(&app) {
                                result = connection.send(c);
                            }
                        }
                        _ => {}
                    }
                }
                Either::Right(Either::Left(update)) => {
                    result = update.and_then(|update| match update {
                        Update::Connected => {
                            let size = resize(&app, &ui);
                            connection.send(size.clone())?;
                            m.sent = Some(size);
                            if !m.live.url.is_empty() {
                                connection.send(Command::Load(m.live.url.clone()))?;
                            }
                            Ok(())
                        }
                        Update::Event(Event::Frame {
                            seq,
                            buffer,
                            width,
                            height,
                            ..
                        }) => {
                            if !m.live.url.is_empty() {
                                let t = e32::time::fast_counter();
                                let image = connection.frame(buffer, width, height)?;
                                ui.global::<BrowserState>().set_frame(image);
                                framed = Some(us_since(t));
                            }
                            connection.send(Command::FrameShown { seq })
                        }
                        Update::Event(Event::Crashed(reason)) => {
                            dprintln!("neoweb: page crashed {}", reason);
                            connection = Connection::Offline;
                            m.crash(&ui, reason);
                            Ok(())
                        }
                        Update::Event(
                            event @ (Event::Keyboard { .. }
                            | Event::Dialog { .. }
                            | Event::Select { .. }
                            | Event::FilePick { .. }
                            | Event::ClipboardSet(_)
                            | Event::ClipboardGet { .. }
                            | Event::Login { .. }),
                        ) => match m.control(&app, &ui, event) {
                            Some(answer) => connection.send(answer),
                            None => Ok(()),
                        },
                        Update::Event(event) => {
                            // The engine may publish its initial blank document before the first Load.
                            if !m.live.url.is_empty() {
                                m.apply(event);
                                m.show(&ui);
                            }
                            Ok(())
                        }
                    });
                }
                Either::Right(Either::Right(())) => {}
            }
            if let Err(error) = result {
                connection = Connection::Offline;
                m.crash(&ui, alloc::format!("engine IPC {}", error));
            }
            while let Some((action, i)) = actions.borrow_mut().pop_front() {
                let command = if action == "upload" {
                    m.pick(&app, i)
                } else {
                    m.act(&ui, &action, i)
                };
                if let Some(command) = command
                    && let Err(error) = connection.send(command)
                {
                    connection = Connection::Offline;
                    m.crash(&ui, alloc::format!("engine IPC {}", error));
                }
                m.show(&ui);
            }
            sync(&app, &ui, &mut m, &mut shown)?;
            // the page area changed (the keyboard came or went): tell the engine once
            let size = resize(&app, &ui);
            if m.sent.as_ref() != Some(&size) && connection.send(size.clone()).is_ok() {
                m.sent = Some(size);
            }
            let t = e32::time::fast_counter();
            app.redraw()?;
            if let Some(copy) = framed.take() {
                stats.add(copy, us_since(t));
            }
        }
    })
}

async fn shell_event(app: &App) -> Result<AppEvent, i32> {
    let mut bytes = [0; ipc::EVENT_LEN];
    let mut descriptor = TPtr8::new(&mut bytes);
    let mut args = TIpcArgs::new();
    args.des8(0, &mut descriptor);
    app.server.send(ipc::NEXT_EVENT, &args).await?;
    AppEvent::decode(&bytes).ok_or(-6)
}

async fn wake(app: &App) {
    match app.wake_us() {
        Some(us) => e32::time::delay(us.clamp(1000, i32::MAX as u64) as i32).await,
        None => pending().await,
    }
}

fn resize(app: &App, ui: &BrowserApp) -> Command {
    Command::Resize {
        w: ui.get_page_width_px() as u32,
        h: ui.get_page_height_px() as u32,
        dpr_milli: (app.dp * 1000.0 + 0.5) as u32,
    }
}

/// Tell the shell what changed: the page (title, `can_back`) and the keyboard a focused field wants (as Contacts does,
/// NeoSymbian/phone `contacts/src/ctx.rs`).
fn sync(app: &App, ui: &BrowserApp, m: &mut Model, shown: &mut Page) -> Result<(), i32> {
    let title = if m.editing {
        "Address"
    } else if !m.showing_web {
        "New tab"
    } else if m.live.crashed {
        "Page crashed"
    } else if m.live.title.is_empty() {
        m.url()
    } else {
        &m.live.title
    };
    let page = Page {
        title: title[..title.floor_char_boundary(ipc::TITLE_MAX)].into(),
        can_back: m.editing || m.showing_web || ui.global::<Kit>().get_layers() > 0,
        ..Page::default()
    };
    if page != *shown {
        app.page(&page)?;
        *shown = page;
    }
    let kf = ui.global::<KeyboardFocus>();
    // a Slint field (address, prompt) first; else the page's focused field (seq -1 marks the page's own)
    let page_keys = m.page_keys > 0 && !m.editing && !ui.get_sheet_open();
    let want = if kf.get_open() || !page_keys {
        (kf.get_open(), kf.get_seq(), kf.get_mode())
    } else {
        (true, -1, i32::from(m.page_keys))
    };
    if want.0 != m.keyboard.0 || (want.0 && (want.1, want.2) != (m.keyboard.1, m.keyboard.2)) {
        m.keyboard = want;
        let mode = if want.0 {
            want.2.max(1)
        } else {
            neoui_proto::ipc::KEYBOARD_HIDDEN
        };
        let h = app.keyboard(mode).unwrap_or(0);
        kf.set_text_h(h as f32);
    }
    Ok(())
}
