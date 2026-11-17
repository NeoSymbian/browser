// SPDX-License-Identifier: GPL-3.0-only
//! NEOWEB.EXE, the browser (docs browser-ui.md). UI0–UI1: the start page and the stand-in engine's pages under the
//! address bar (secure/not secure, loading with ✕, edit mode with suggestions) and the five-icon footer; the chrome
//! hides while reading; Back walks edit mode, then the page history (`can_back`).
#![no_std]

extern crate alloc;

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use e32::dprintln;
use neoapp::{App, AppEvent, Page};
use neoweb_core::chrome::{ChromeScroll, Loading};
use neoweb_core::input::{self, Suggestion};
use neoweb_core::{Engine, FakeEngine, FakePage, Header, START, Tab, count_label, start};
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
    tab: Tab,
    engine: FakeEngine,
    visited: Vec<(String, String)>,
    editing: bool,
    suggestions: Vec<Suggestion>,
    scroll: ChromeScroll,
    loading: Loading,
    /// the keyboard last asked of the UI server: (open, KeyboardFocus.seq, mode)
    keyboard: (bool, i32, i32),
}

impl Model {
    fn go(&mut self, ui: &BrowserApp, url: String) {
        self.end_edit(ui);
        self.tab.open(&url);
        let title = self.engine.load(&url).title;
        self.visited.push((title, url));
        self.arrive(ui);
    }

    /// A page arrived (opened, ←, →, reload): it loads from its top with the chrome shown.
    fn arrive(&mut self, ui: &BrowserApp) {
        self.loading.start();
        self.scroll.show(0.0);
        ui.global::<BrowserState>().set_page_y(0.0);
    }

    fn end_edit(&mut self, ui: &BrowserApp) {
        self.editing = false;
        ui.global::<KeyboardFocus>().invoke_close();
    }

    fn act(&mut self, ui: &BrowserApp, action: &str, i: i32) {
        let s = ui.global::<BrowserState>();
        match action {
            "shortcut" => {
                if let Some(t) = start::SHORTCUTS.get(i as usize) {
                    self.go(ui, t.url.into());
                }
            }
            "recent" => {
                let urls = self.visited.iter().map(|(_, u)| u.as_str());
                if let Some(r) = start::recent(urls).get(i as usize) {
                    let url = alloc::format!("https://{r}");
                    self.go(ui, url);
                }
            }
            "address" | "search" => {
                let url = self.tab.current();
                s.set_edit_text(if url == START { "".into() } else { url.into() });
                self.editing = true;
                self.suggest(&s.get_edit_text());
            }
            "edited" => self.suggest(&s.get_edit_text()),
            "go" => {
                let text = s.get_edit_text();
                if !text.trim().is_empty() {
                    self.go(ui, input::parse(&text));
                }
            }
            "suggestion" => {
                let url = match self.suggestions.get(i as usize) {
                    Some(Suggestion::History(_, u) | Suggestion::Bookmark(_, u)) => u.clone(),
                    Some(Suggestion::Search(t)) => input::search(t),
                    None => return,
                };
                self.go(ui, url);
            }
            "cancel-edit" => self.end_edit(ui),
            "back" => {
                if self.tab.back() {
                    self.arrive(ui);
                }
            }
            "forward" => {
                if self.tab.forward() {
                    self.arrive(ui);
                }
            }
            "reload" => self.arrive(ui),
            "stop" => self.loading.stop(),
            "tick" => {
                self.loading.tick();
            }
            "scroll" => {
                self.scroll.scrolled(i as f32);
            }
            // tabs, Options, bookmarks, site info, edit shortcuts: UI2–UI5
            _ => {}
        }
    }

    fn suggest(&mut self, text: &str) {
        self.suggestions = input::suggest(text, &self.visited, &[]);
    }

    /// Back (`can_back` was true): edit mode first, then the page history (browser-ui.md "Back order").
    fn back(&mut self, ui: &BrowserApp) {
        if self.editing {
            self.end_edit(ui);
        } else if self.tab.back() {
            self.arrive(ui);
        }
    }

    fn can_back(&self) -> bool {
        self.editing || self.tab.can_back()
    }

    /// Put the model into the UI.
    fn show(&self, ui: &BrowserApp) {
        let url = self.tab.current();
        let page = FakeEngine.load(url).page;
        let header = Header::of(url);
        let s = ui.global::<BrowserState>();
        s.set_page(page as i32);
        s.set_security(header.security as i32);
        s.set_domain(header.domain.as_str().into());
        s.set_can_back(self.tab.can_back());
        s.set_can_forward(self.tab.can_forward());
        s.set_count(count_label(1).as_str().into());
        s.set_loading(self.loading.active());
        s.set_progress(self.loading.progress());
        s.set_editing(self.editing);
        // B02: always shown while loading, editing and on the start page
        let shown =
            self.scroll.shown() || self.loading.active() || self.editing || page == FakePage::Start;
        s.set_chrome_shown(shown);
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
    let m = Rc::new(RefCell::new(Model::default()));
    m.borrow().show(&ui);
    {
        let (weak, m) = (ui.as_weak(), m.clone());
        ui.global::<BrowserState>()
            .on_act(move |action: SharedString, i| {
                let Some(ui) = weak.upgrade() else { return };
                let mut m = m.borrow_mut();
                m.act(&ui, &action, i);
                m.show(&ui);
            });
    }
    ui.show().map_err(|_| -2)?;
    let mut shown = Page::default();
    sync(&app, &ui, &mut m.borrow_mut(), &mut shown)?;
    app.draw()?;
    app.run(|e| {
        match e {
            AppEvent::Back => {
                let mut mm = m.borrow_mut();
                mm.back(&ui);
                mm.show(&ui);
            }
            AppEvent::Keyboard { h } => {
                // 0 while the field keeps the focus: the user put the keyboard away; a tap on the field asks again
                let kf = ui.global::<KeyboardFocus>();
                kf.set_dismissed(h == 0 && kf.get_open());
                kf.set_text_h(h as f32);
            }
            _ => {}
        }
        sync(&app, &ui, &mut m.borrow_mut(), &mut shown).map(|()| true)
    })
}

/// Tell the shell what changed: the page (title, `can_back`) and the keyboard a focused field wants (as Contacts does,
/// NeoSymbian/phone `contacts/src/ctx.rs`).
fn sync(app: &App, ui: &BrowserApp, m: &mut Model, shown: &mut Page) -> Result<(), i32> {
    let mut title = if m.editing {
        String::from("Address")
    } else {
        FakeEngine.load(m.tab.current()).title
    };
    title.truncate(48);
    let page = Page {
        title,
        can_back: m.can_back(),
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
