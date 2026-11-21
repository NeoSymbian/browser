// SPDX-License-Identifier: GPL-3.0-only
//! NEOWEB.EXE owns the browser chrome and displays frames from NEOWEBE.EXE.
#![no_std]

extern crate alloc;

mod engine;
mod library;

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
use neoweb_core::tabs::{Change, Tabs};
use neoweb_core::{Header, LiveTab, START, count_label, start};
use neoweb_proto::{Command, DialogKind, Event, PressKind, SelectItem, SelectOp, TouchKind};
use neoweb_ui::{
    BrowserApp, BrowserState, KeyboardFocus, Kit, SelectRow, ShortcutTile, Spot, Suggest, TabCard,
    Theme,
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
    /// a library prompt (new or renamed folder, renamed bookmark), or a bookmark's Move choice (UI4)
    Rename(library::Rename),
    Move(usize),
}

/// A finger held still on the page: a long-press after [`PRESS_MS`] (WB5c).
#[derive(Clone, Copy)]
struct Press {
    id: u8,
    x: i32,
    y: i32,
    at: u64,
}

const PRESS_MS: u64 = 500;

/// The opened pages (UI2), the shown tab's page, the pages visited (title, address; oldest first), the chrome.
#[derive(Default)]
struct Model {
    tabs: Tabs,
    /// small copies of tabs' last frames (Opened pages), and the frame shown now
    thumbs: Vec<(u32, slint::Image)>,
    frame: Option<slint::Image>,
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
    /// the site a Save password offer names
    offer_site: String,
    /// uploads picked (each in its own folder: two files of one name stay apart)
    picks: u32,
    /// sites the user said Never to (Save password)
    never_save: Vec<String>,
    /// the page size last sent to the engine
    sent: Option<Command>,
    /// commands for the engine, sent after each event (WB5c)
    out: Vec<Command>,
    /// a NOTE to show after this event
    note: Option<&'static str>,
    /// a long-press in the making; a finger the page doesn't get (long-pressed, or dragging a selection end)
    press: Option<Press>,
    taken: Option<u8>,
    /// the selection end being dragged; the selection's ends (page px, Event::Selection)
    drag: Option<SelectOp>,
    selection: Vec<i32>,
    finding: bool,
    /// the next copied text is a search (the selection bar's Search), not for the clipboard
    search_copy: bool,
    desktop: bool,
    /// bookmarks, history, downloads, settings (UI4)
    lib: library::Library,
    /// the shell's QUERY is "Clear browsing data?"
    clear_query: bool,
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
        let Some(tab) = self.tabs.current else {
            self.switch(ui, |t| t.open(&url));
            return None;
        };
        self.tabs.info(tab, &url, "");
        Some(Command::Open { tab, url })
    }

    /// Change the tabs with `f`: keep the shown page's thumbnail, free evicted or closed pages, show the new
    /// current tab (its page made again with its address when it has none).
    fn switch(&mut self, ui: &BrowserApp, f: impl FnOnce(&mut Tabs) -> Option<Change>) {
        if let (Some(cur), Some(img)) = (self.tabs.current, self.frame.take()) {
            self.thumbs.retain(|(t, _)| *t != cur);
            self.thumbs.push((cur, thumb(&img)));
        }
        match f(&mut self.tabs) {
            Some(c) => self.apply_change(ui, c),
            None => self.note = Some("30 pages are open: close one first"),
        }
    }

    fn apply_change(&mut self, ui: &BrowserApp, c: Change) {
        for tab in c.free {
            self.out.push(Command::Close { tab });
        }
        self.thumbs.retain(|(t, _)| self.tabs.get(*t).is_some());
        let Some((tab, reload)) = c.show else {
            if self.tabs.current.is_none()
                && let Some(c) = self.tabs.open(START)
            {
                // the last tab closed: a new one with the start page
                self.apply_change(ui, c);
            }
            return;
        };
        let t = self.tabs.get(tab).cloned().unwrap_or_default();
        self.live = LiveTab {
            url: t.url.clone(),
            title: t.title.clone(),
            ..LiveTab::default()
        };
        self.showing_web = !t.url.is_empty() && t.url != START;
        self.selection.clear();
        self.finding = false;
        self.end_edit(ui);
        ui.global::<BrowserState>()
            .set_frame(slint::Image::default());
        match reload {
            Some(url) if self.showing_web => {
                if self.thumbs.iter().any(|(id, _)| *id == tab) {
                    self.note = Some("Page reloaded to save memory");
                }
                self.out.push(Command::Open { tab, url });
            }
            Some(_) => {} // the start page has no engine page
            None => self.out.push(Command::Activate { tab }),
        }
    }

    /// The link (or the page) to share: on the clipboard, and Messages opened to write (it takes no text yet).
    fn share(&mut self, app: &App, url: &str) {
        if app.set_clipboard(url) {
            let _ = app.open("messages", 3, ""); // phone-core page::messages::NEW
            self.note = Some("Link copied: paste it in the message");
        }
    }

    fn copy_link(&mut self, app: &App, url: &str) {
        self.note = Some(if app.set_clipboard(url) {
            "Link copied"
        } else {
            "The link is too long to copy"
        });
    }

    /// UI2's actions (WB5c): tabs, Options, the link menu, the selection, find. True when `action` was one.
    fn act2(&mut self, app: &App, ui: &BrowserApp, action: &str, i: i32) -> bool {
        let s = ui.global::<BrowserState>();
        let nth = |m: &Self| m.tabs.list.get(i as usize).map(|t| t.id);
        match action {
            "tabs" => {
                self.show_tabs(ui);
                s.set_tabs_open(true);
            }
            "tab" => {
                if let Some(id) = nth(self) {
                    self.switch(ui, |t| Some(t.show(id)));
                }
            }
            "tab-close" => {
                if let Some(id) = nth(self) {
                    self.switch(ui, |t| Some(t.close(id)));
                    self.show_tabs(ui);
                }
            }
            "new-tab" => self.switch(ui, |t| t.open(START)),
            "tabs-options" | "options" => ui.invoke_show_sheet(6),
            "tabs-back" => {}
            "option" => match i {
                0 => self.switch(ui, |t| t.open(START)),
                1 if self.showing_web => {
                    self.finding = true;
                    s.set_find_text("".into());
                    s.set_find_count("".into());
                }
                2 if self.showing_web => self.share(app, &self.live.url.clone()),
                3 if self.showing_web => self.copy_link(app, &self.live.url.clone()),
                4 => {
                    self.desktop = !self.desktop;
                    self.out.push(Command::DesktopSite(self.desktop));
                    if self.showing_web {
                        self.out.push(Command::Reload);
                    }
                }
                5..=8 => {
                    ui.invoke_show_library(i - 4);
                    self.lib.show(ui);
                }
                _ => {}
            },
            "bookmark" if self.showing_web => {
                let on = self
                    .lib
                    .toggle_bookmark(&self.live.title.clone(), &self.live.url.clone());
                self.note = Some(if on {
                    "Added to bookmarks"
                } else {
                    "Bookmark removed"
                });
            }
            a if a.starts_with("bm-")
                || a.starts_with("hist-")
                || a.starts_with("dl-")
                || a.starts_with("set-")
                || a.starts_with("library-") =>
            {
                let mut out = Vec::new();
                let then = self.lib.act(app, ui, a, i, &mut out);
                self.out.extend(out);
                match then {
                    library::Then::Nothing => {}
                    library::Then::Go(url) => {
                        ui.invoke_show_library(0);
                        if let Some(c) = self.go(ui, url) {
                            self.out.push(c);
                        }
                    }
                    library::Then::AskClear => {
                        let q = Query {
                            text: "Clear browsing data?\nHistory, cookies and cached files on this phone are deleted."
                                .into(),
                            buttons: alloc::vec!["Clear".into(), "Cancel".into()],
                            destructive: true,
                        };
                        self.clear_query = app.query(&q).is_ok();
                    }
                    library::Then::Prompt(what, question, value) => {
                        s.set_sheet_site("Browser".into());
                        s.set_sheet_text(question.into());
                        s.set_sheet_value(value.as_str().into());
                        self.control = Control::Rename(what);
                        ui.invoke_show_sheet(1);
                    }
                    library::Then::Move(b) => {
                        let items: Vec<SelectItem> = self
                            .lib
                            .folder_names()
                            .into_iter()
                            .enumerate()
                            .map(|(n, label)| SelectItem {
                                id: n as u32,
                                label,
                                disabled: false,
                                selected: false,
                            })
                            .collect();
                        s.set_select_rows(select_rows(&items));
                        s.set_select_multiple(false);
                        self.control = Control::Move(b);
                        ui.invoke_show_sheet(2);
                    }
                }
                self.lib.show(ui);
            }
            "link" => {
                let url: String = s.get_link_url().into();
                match i {
                    0 => self.switch(ui, |t| t.open(&url)),
                    1 => self.copy_link(app, &url),
                    2 => {
                        self.out.push(Command::Download { url });
                        self.note = Some("Downloading…");
                    }
                    3 => self.share(app, &url),
                    _ => {}
                }
            }
            "link-cancel" => {}
            "selection" => {
                self.search_copy = i == 2;
                let op = if i == 1 {
                    SelectOp::SelectAll
                } else {
                    SelectOp::Copy
                };
                self.out.push(Command::Select { op, x: 0, y: 0 });
                if i != 1 {
                    self.out.push(Command::Select {
                        op: SelectOp::Clear,
                        x: 0,
                        y: 0,
                    });
                }
            }
            "find-edited" | "find-next" | "find-prev" => {
                let dir = match action {
                    "find-next" => 1,
                    "find-prev" => -1,
                    _ => 0,
                };
                self.out.push(Command::Find {
                    text: s.get_find_text().into(),
                    dir,
                });
            }
            "find-close" => self.end_find(ui),
            _ => return false,
        }
        true
    }

    fn end_find(&mut self, ui: &BrowserApp) {
        if self.finding {
            self.finding = false;
            ui.global::<KeyboardFocus>().invoke_close();
            self.out.push(Command::Find {
                text: String::new(),
                dir: 0,
            });
        }
    }

    /// Opened pages: a card per tab, the current one with its live frame.
    fn show_tabs(&self, ui: &BrowserApp) {
        let cards: Vec<TabCard> = self
            .tabs
            .list
            .iter()
            .map(|t| {
                let current = self.tabs.current == Some(t.id);
                let domain = Header::of(&t.url).domain;
                let title = if !t.title.is_empty() {
                    t.title.as_str()
                } else if t.url == START || t.url.is_empty() {
                    "New tab"
                } else {
                    domain.as_str()
                };
                let thumb = match (current, &self.frame) {
                    (true, Some(f)) => thumb(f),
                    _ => self
                        .thumbs
                        .iter()
                        .find(|(id, _)| *id == t.id)
                        .map(|(_, i)| i.clone())
                        .unwrap_or_default(),
                };
                TabCard {
                    title: title.into(),
                    letter: SharedString::from(String::from(
                        domain
                            .chars()
                            .next()
                            .map_or('+', |c| c.to_ascii_uppercase()),
                    )),
                    color: slint::Color::from_argb_encoded(badge(&domain)),
                    thumb,
                    current,
                }
            })
            .collect();
        ui.global::<BrowserState>().set_tabs(model(cards));
    }

    /// The selection's ends from the engine: the handles and the bar (above the start, or under the end).
    fn selection(&mut self, app: &App, ui: &BrowserApp, ends: Vec<i32>) {
        let s = ui.global::<BrowserState>();
        self.selection = ends;
        let [ax, ay, bx, by, line] = self.selection[..] else {
            s.set_selection_shown(false);
            return;
        };
        let top = ui.get_page_top_px();
        let dp = app.dp;
        s.set_handle_a(Spot {
            x: ax as f32,
            y: (ay + top) as f32,
        });
        s.set_handle_b(Spot {
            x: bx as f32,
            y: (by + top) as f32,
        });
        let bar_h = 40.0 * dp;
        let above = (ay + top) as f32 - bar_h - 12.0 * dp;
        s.set_selection_y(if above > top as f32 {
            above
        } else {
            (by + top + line) as f32 + 40.0 * dp
        });
        s.set_selection_x(ax as f32 - 60.0 * dp);
        s.set_selection_shown(true);
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
                    let url = input::parse(&text);
                    // a search goes to the engine Settings chose
                    let url = if url == input::search(&text) {
                        self.lib.search(&text)
                    } else {
                        url
                    };
                    return self.go(ui, url);
                }
            }
            "suggestion" => {
                let url = match self.suggestions.get(i as usize) {
                    Some(Suggestion::History(_, u) | Suggestion::Bookmark(_, u)) => u.clone(),
                    Some(Suggestion::Search(t)) => self.lib.search(t),
                    None => return None,
                };
                return self.go(ui, url);
            }
            "cancel-edit" => self.end_edit(ui),
            "back" if self.finding => self.end_find(ui),
            "back" if !self.selection.is_empty() => {
                return Some(Command::Select {
                    op: SelectOp::Clear,
                    x: 0,
                    y: 0,
                });
            }
            "back" => match controls::back_step(self.editing, self.showing_web, self.live.back) {
                BackStep::EndEdit => self.end_edit(ui),
                BackStep::History => return Some(Command::Back),
                BackStep::StartPage | BackStep::Shell => {
                    // a page's new window goes back to the page that opened it
                    let opener = self.tabs.current().and_then(|t| t.opener);
                    match (opener, self.tabs.current) {
                        (Some(o), Some(cur)) if self.tabs.get(o).is_some() => {
                            self.switch(ui, |t| {
                                let mut c = t.close(cur);
                                let s = t.show(o);
                                c.show = s.show;
                                c.free.extend(s.free);
                                Some(c)
                            });
                        }
                        _ => self.showing_web = false,
                    }
                }
            },
            "forward" if !self.showing_web => self.showing_web = !self.live.url.is_empty(),
            "forward" if self.live.forward => return Some(Command::Forward),
            "reload" if self.live.crashed => return self.go(ui, self.live.url.clone()),
            "reload" if self.showing_web => return Some(Command::Reload),
            "stop" => {
                self.live.loading.stop();
                return Some(Command::Stop);
            }
            "prompt" => match core::mem::take(&mut self.control) {
                Control::Prompt(id) => {
                    return Some(Command::Answer {
                        id,
                        ok: i == 1,
                        text: s.get_sheet_value().into(),
                    });
                }
                Control::Rename(r) if i == 1 => {
                    self.lib.renamed(r, &s.get_sheet_value());
                    self.lib.show(ui);
                }
                _ => {}
            },
            "select" if matches!(self.control, Control::Move(_)) => {
                if let Control::Move(b) = core::mem::take(&mut self.control) {
                    self.lib.moved(b, i.max(0) as usize);
                    self.lib.show(ui);
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
                let site = core::mem::take(&mut self.offer_site);
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
        // a dialog names its site; a page on the phone itself has none
        let site = match Header::of(&self.live.url).domain {
            d if d.is_empty() || d.contains('/') => String::from("This page"),
            d => d,
        };
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
            // one shell QUERY at a time: "Clear browsing data?" is open, so the page's dialog is cancelled (review #2)
            Event::Dialog { id, .. } if self.clear_query => {
                return Some(Command::Answer {
                    id,
                    ok: false,
                    text: String::new(),
                });
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
            Event::Pressed {
                kind: PressKind::Link | PressKind::Image,
                title,
                url,
                y,
            } => {
                s.set_link_title(title.as_str().into());
                s.set_link_url(url.as_str().into());
                s.set_link_y((y + ui.get_page_top_px()) as f32);
                ui.invoke_show_sheet(5);
            }
            Event::Pressed { .. } => {}
            Event::Selection { ends } => self.selection(app, ui, ends),
            Event::Found { index, count } => {
                s.set_find_count(alloc::format!("{index}/{count}").as_str().into());
            }
            Event::TabInfo { tab, url, title } => self.tabs.info(tab, &url, &title),
            e @ Event::Download { .. } => {
                self.lib.download(app, e);
                if ui.global::<BrowserState>().get_library() == 3 {
                    self.lib.show(ui);
                }
            }
            Event::Created { tab, opener } => {
                if let (Some(cur), Some(img)) = (self.tabs.current, self.frame.take()) {
                    self.thumbs.retain(|(t, _)| *t != cur);
                    self.thumbs.push((cur, thumb(&img)));
                }
                let c = self.tabs.created(tab, opener);
                for tab in c.free {
                    self.out.push(Command::Close { tab });
                }
                self.live = LiveTab::default();
                self.showing_web = true;
                self.selection.clear();
            }
            Event::ClipboardSet(text) if core::mem::take(&mut self.search_copy) => {
                let url = input::search(text.trim());
                self.switch(ui, |t| t.open(&url));
            }
            Event::ClipboardSet(text) => {
                // over the shell's 2048 bytes the copy is refused, never cut (browser/ui.md)
                if !app.set_clipboard(&text) {
                    let _ = app.note("Too long to copy", "");
                }
            }
            Event::Login { user } => {
                // B11: no secure store yet, so the sheet only says it; Never is remembered for this run. Not over
                // another sheet, whose request would lose its answer (review #5); Never takes the site shown (#12).
                if !self.never_save.contains(&site) && !ui.get_sheet_open() {
                    self.offer_site = site.clone();
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
        let mut filter = controls::picker_filter(&accept, i == 0);
        // the shell's OPEN takes 256 bytes of argument: a longer filter would cancel the picker (review #11)
        if filter.len() > ipc::ARG_MAX {
            filter.clear();
        }
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
        self.picks += 1;
        let pick = self.picks;
        let paths = match app.result_file().map(|f| copy_for_engine(&f, pick)) {
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
        self.suggestions =
            input::suggest(text, &self.lib.history.pairs(), &self.lib.bookmarks.list);
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
        if visit && let Some(tab) = self.tabs.current {
            self.tabs.info(tab, &self.live.url, &self.live.title);
        }
        if visit {
            self.lib.visited(&self.live.title, &self.live.url);
        }
        if visit && !self.live.url.is_empty() {
            match self.visited.last_mut() {
                Some((title, url)) if *url == self.live.url => title.clone_from(&self.live.title),
                _ => self
                    .visited
                    .push((self.live.title.clone(), self.live.url.clone())),
            }
        }
    }

    /// The long-press timer ran out: the page's touch is cancelled and the engine says what was pressed.
    fn long_press(&mut self) {
        let Some(p) = self.press.take() else { return };
        self.taken = Some(p.id);
        self.out.push(Command::Touch {
            kind: TouchKind::Cancel,
            id: p.id.into(),
            x: p.x,
            y: p.y,
        });
        self.out.push(Command::Press { x: p.x, y: p.y });
    }

    /// Capture a finger at press time so a drag across the toolbar still ends in the page.
    fn touch(&mut self, ui: &BrowserApp, input: InputEvent) -> Option<Command> {
        let InputEvent::Touch { id, x, y, phase } = input else {
            return None;
        };
        let y = y - ui.get_page_top_px();
        // an open sheet, Opened pages or a library view takes the touches (Codex review #1), and so does the
        // selection bar's band (its y is in surface px; `y` here is the page's)
        let st = ui.global::<BrowserState>();
        let bar_y = st.get_selection_y() as i32;
        let bar = (40.0 * ui.global::<Theme>().get_dp()) as i32;
        let on_bar =
            st.get_selection_shown() && (bar_y..bar_y + bar).contains(&(y + ui.get_page_top_px()));
        let covered = ui.get_sheet_open() || st.get_tabs_open() || st.get_library() != 0 || on_bar;
        let in_page = self.showing_web
            && !self.live.crashed
            && !self.editing
            && !covered
            && (0..ui.get_page_width_px()).contains(&x)
            && (0..ui.get_page_height_px()).contains(&y);
        // a finger taken from the page: dragging a selection end, or after a long-press
        if self.taken == Some(id) {
            match (phase, self.drag) {
                (TouchPhase::Move, Some(op)) => self.out.push(Command::Select { op, x, y }),
                (TouchPhase::Up, _) => (self.taken, self.drag) = (None, None),
                _ => {}
            }
            return None;
        }
        if in_page && phase == TouchPhase::Down {
            // a selection end's knob (26 dp under the line's top) takes the finger
            let grab = (28.0 * ui.global::<Theme>().get_dp()) as i32;
            let near = |px: i32, py: i32| (x - px).abs() < grab && (y - (py + grab)).abs() < grab;
            if let [ax, ay, bx, by, _] = self.selection[..] {
                let op = if near(ax, ay) {
                    Some(SelectOp::DragStart)
                } else if near(bx, by) {
                    Some(SelectOp::DragEnd)
                } else {
                    None
                };
                if op.is_some() {
                    (self.taken, self.drag) = (Some(id), op);
                    return None;
                }
                // a tap elsewhere ends the selection (and goes to the page)
                self.out.push(Command::Select {
                    op: SelectOp::Clear,
                    x: 0,
                    y: 0,
                });
            }
            self.press = Some(Press {
                id,
                x,
                y,
                at: e32::time::millis(),
            });
        }
        if let Some(p) = self.press
            && p.id == id
        {
            let slop = (10.0 * ui.global::<Theme>().get_dp()) as i32;
            if phase == TouchPhase::Up || (p.x - x).abs() > slop || (p.y - y).abs() > slop {
                self.press = None;
            }
        }
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
        s.set_count(count_label(self.tabs.list.len()).as_str().into());
        s.set_finding(self.finding);
        s.set_bookmarked(self.showing_web && self.lib.bookmarks.is_bookmarked(&self.live.url));
        s.set_desktop_site(self.desktop);
        if self.selection.is_empty() {
            s.set_selection_shown(false);
        }
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

/// A sixth of a frame, for Opened pages' cards.
fn thumb(img: &slint::Image) -> slint::Image {
    let Some(src) = img.to_rgb8() else {
        return slint::Image::default();
    };
    let (w, h) = (src.width() / 6, src.height() / 6);
    let mut dst = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(w, h);
    let from = src.as_slice();
    let sw = src.width() as usize;
    for (i, px) in dst.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = (i % w as usize, i / w as usize);
        *px = from[y * 6 * sw + x * 6];
    }
    slint::Image::from_rgb8(dst)
}

/// A tab badge's colour from its site's name.
fn badge(domain: &str) -> u32 {
    const COLORS: [u32; 6] = [0x5b6472, 0x24292f, 0x2f80ed, 0x22b46a, 0xe2a92a, 0xe5484d];
    let h = domain
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b.into()));
    0xff00_0000 | COLORS[h as usize % COLORS.len()]
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
fn copy_for_engine(f: &e32::fs::SharedFile, pick: u32) -> Result<String, i32> {
    use e32::fs::{Fs, mode};
    let src = f.file();
    // a folder per pick keeps two files of one name apart (Servo reads the path again when the form is sent)
    let path = controls::upload_path(&src.full_name()?).replacen(
        controls::UPLOAD_DIR,
        &alloc::format!("{}{}/", controls::UPLOAD_DIR, pick),
        1,
    );
    let fs = Fs::connect()?;
    let sym: String = path.replace('/', "\\");
    let dir = &sym[..=sym.rfind('\\').unwrap_or(0)];
    // the folders may exist
    let _ = fs.mkdir(&controls::UPLOAD_DIR.replace('/', "\\"));
    let _ = fs.mkdir(dir);
    let copy = || -> Result<(), i32> {
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
        dst.flush()
    };
    // shortcut: the copy runs on the UI loop (a big file stalls the screen); a thread if uploads get large
    copy().inspect_err(|_| {
        let _ = fs.delete(&sym); // no partial file left behind (review #8)
    })?;
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
    m.lib = library::Library::load();
    m.desktop = m.lib.settings.desktop_site;
    m.tabs.open(START);
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
            let pressing = m.press.map(|p| p.at + PRESS_MS);
            let event = rt::select(
                next.as_mut(),
                rt::select(
                    connection.next(),
                    rt::select(wake(&app), press_timer(pressing)),
                ),
            )
            .await;
            let mut result = Ok(());
            match event {
                Either::Left(event) => {
                    let event = event?;
                    next.set(shell_event(&app));
                    if event == AppEvent::Quit {
                        return Ok(());
                    }
                    let page_typing =
                        m.page_keys > 0 && !m.editing && !m.finding && !ui.get_sheet_open();
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
                        AppEvent::Answer { i } if core::mem::take(&mut m.clear_query) => {
                            if i == 0 {
                                m.lib.cleared();
                                m.visited.clear(); // the start page's recents (review #14)
                                m.out.push(Command::ClearData);
                                m.note = Some("Browsing data cleared");
                            }
                        }
                        AppEvent::Notified { id, action } => {
                            m.lib.notified(&app, &ui, id, action.into())
                        }
                        AppEvent::Open { page: 3 } => {
                            ui.invoke_show_library(3);
                            m.lib.show(&ui);
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
                            for c in m.lib.engine_settings() {
                                connection.send(c)?;
                            }
                            // a new engine has no pages: the current tab's is made again, the others reload when shown
                            m.tabs.forget_pages();
                            if let Some(tab) = m.tabs.current
                                && m.showing_web
                            {
                                m.tabs.show(tab);
                                connection.send(Command::Open {
                                    tab,
                                    url: m.live.url.clone(),
                                })?;
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
                            if m.showing_web {
                                let t = e32::time::fast_counter();
                                let image = connection.frame(buffer, width, height)?;
                                m.frame = Some(image.clone());
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
                            | Event::Login { .. }
                            | Event::Pressed { .. }
                            | Event::Selection { .. }
                            | Event::Found { .. }
                            | Event::TabInfo { .. }
                            | Event::Created { .. }
                            | Event::Download { .. }),
                        ) => match m.control(&app, &ui, event) {
                            Some(answer) => connection.send(answer),
                            None => Ok(()),
                        },
                        Update::Event(event) => {
                            // The engine may publish its initial blank document before the first Load: only a shown
                            // page takes events (an empty address never blanks the one typed).
                            if m.showing_web && !matches!(&event, Event::Url(u) if u.is_empty()) {
                                m.apply(event);
                                m.show(&ui);
                            }
                            Ok(())
                        }
                    });
                }
                Either::Right(Either::Right(Either::Left(()))) => {}
                Either::Right(Either::Right(Either::Right(()))) => m.long_press(),
            }
            if let Err(error) = result {
                connection = Connection::Offline;
                m.crash(&ui, alloc::format!("engine IPC {}", error));
            }
            // the borrow ends before the action runs: an action can make Slint queue another (Codex WB5d review #1)
            loop {
                let next = actions.borrow_mut().pop_front();
                let Some((action, i)) = next else { break };
                dprintln!("neoweb: act {} {}", action, i);
                let command = if action == "upload" {
                    m.pick(&app, i)
                } else if m.act2(&app, &ui, &action, i) {
                    None
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
            for command in core::mem::take(&mut m.out) {
                if let Err(error) = connection.send(command) {
                    connection = Connection::Offline;
                    m.crash(&ui, alloc::format!("engine IPC {}", error));
                    break;
                }
            }
            if let Some(note) = m.note.take() {
                let _ = app.note(note, "");
            }
            m.lib.save();
            m.show(&ui);
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

/// The long-press deadline (ms on `e32::time::millis`), if a finger is held.
async fn press_timer(at: Option<u64>) {
    match at {
        Some(at) => {
            let left = at.saturating_sub(e32::time::millis()).max(1);
            e32::time::delay((left * 1000).min(i32::MAX as u64) as i32).await
        }
        None => pending().await,
    }
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
    let page_keys = m.page_keys > 0 && !m.editing && !m.finding && !ui.get_sheet_open();
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
        // the shell answers with the height now and sends Keyboard { h } only on a later change (review #6)
        ui.global::<BrowserState>().set_keyboard_px(h as i32);
    }
    Ok(())
}
