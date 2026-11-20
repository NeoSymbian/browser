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
use neoapp::{App, AppEvent, Page};
use neoui_proto::{Fingers, InputEvent, TouchPhase, ipc};
use neoweb_core::input::{self, Suggestion};
use neoweb_core::{Header, LiveTab, START, count_label, start};
use neoweb_proto::{Command, Event, TouchKind};
use neoweb_ui::{BrowserApp, BrowserState, KeyboardFocus, ShortcutTile, Suggest, Theme};
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
            "back" if self.editing => self.end_edit(ui),
            "back" if self.live.back && self.showing_web => return Some(Command::Back),
            "back" => self.showing_web = false,
            "forward" if !self.showing_web => self.showing_web = !self.live.url.is_empty(),
            "forward" if self.live.forward => return Some(Command::Forward),
            "reload" if self.live.crashed => return self.go(ui, self.live.url.clone()),
            "reload" if self.showing_web => return Some(Command::Reload),
            "stop" => {
                self.live.loading.stop();
                return Some(Command::Stop);
            }
            // tabs, Options, bookmarks, site info, edit shortcuts: UI2–UI5
            _ => {}
        }
        None
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
                    if let AppEvent::Input(input) = event
                        && let Some(command) = m.touch(&ui, input)
                    {
                        result = connection.send(command);
                    } else {
                        app.handle(&event);
                    }
                    match event {
                        AppEvent::Back => actions.borrow_mut().push_back(("back".into(), 0)),
                        AppEvent::Area { .. } => result = connection.send(resize(&app, &ui)),
                        AppEvent::Keyboard { h } => {
                            let kf = ui.global::<KeyboardFocus>();
                            kf.set_dismissed(h == 0 && kf.get_open());
                            kf.set_text_h(h as f32);
                        }
                        _ => {}
                    }
                }
                Either::Right(Either::Left(update)) => {
                    result = update.and_then(|update| match update {
                        Update::Connected => {
                            connection.send(resize(&app, &ui))?;
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
                                let image = connection.frame(buffer, width, height)?;
                                ui.global::<BrowserState>().set_frame(image);
                            }
                            connection.send(Command::FrameShown { seq })
                        }
                        Update::Event(Event::Crashed(reason)) => {
                            dprintln!("neoweb: page crashed {}", reason);
                            connection = Connection::Offline;
                            m.crash(&ui, reason);
                            Ok(())
                        }
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
                if let Some(command) = m.act(&ui, &action, i)
                    && let Err(error) = connection.send(command)
                {
                    connection = Connection::Offline;
                    m.crash(&ui, alloc::format!("engine IPC {}", error));
                }
                m.show(&ui);
            }
            sync(&app, &ui, &mut m, &mut shown)?;
            app.redraw()?;
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
        can_back: m.editing || m.showing_web,
        ..Page::default()
    };
    if page != *shown {
        app.page(&page)?;
        *shown = page;
    }
    let kf = ui.global::<KeyboardFocus>();
    let want = (kf.get_open(), kf.get_seq(), kf.get_mode());
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
