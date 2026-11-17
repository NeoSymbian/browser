// SPDX-License-Identifier: GPL-3.0-only
//! NEOWEB.EXE, the browser (docs browser-ui.md). UI0: the start page and pages of the stand-in engine under the
//! address bar and the five-icon footer; ← → and the system Back walk the page history (`can_back`).
#![no_std]

extern crate alloc;

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use e32::dprintln;
use neoapp::{App, AppEvent, Page};
use neoweb_core::{Engine, FakeEngine, Header, Tab, count_label, start};
use neoweb_ui::{BrowserApp, BrowserState, ShortcutTile, Theme};
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

/// One tab for now (Opened pages is UI2) and the addresses visited, for Recently visited.
#[derive(Default)]
struct Model {
    tab: Tab,
    engine: FakeEngine,
    visited: Vec<String>,
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
    ui.global::<BrowserState>()
        .set_shortcuts(ModelRc::from(Rc::new(VecModel::from(tiles))));
    let model = Rc::new(RefCell::new(Model::default()));
    show(&ui, &mut model.borrow_mut());
    {
        let (weak, model) = (ui.as_weak(), model.clone());
        ui.global::<BrowserState>()
            .on_act(move |action: SharedString, i| {
                let Some(ui) = weak.upgrade() else { return };
                let mut m = model.borrow_mut();
                let url = match action.as_str() {
                    "shortcut" => start::SHORTCUTS
                        .get(i as usize)
                        .map(|s| String::from(s.url)),
                    "recent" => start::recent(m.visited.iter().map(String::as_str))
                        .get(i as usize)
                        .map(|r| alloc::format!("https://{r}")),
                    "back" => {
                        m.tab.back();
                        None
                    }
                    "forward" => {
                        m.tab.forward();
                        None
                    }
                    // the address bar, tabs, Options, bookmarks: UI1–UI4
                    _ => None,
                };
                if let Some(url) = url {
                    m.tab.open(&url);
                    m.visited.push(url);
                }
                show(&ui, &mut m);
            });
    }
    ui.show().map_err(|_| -2)?;
    let mut shown = Page::default();
    describe(&app, &model.borrow(), &mut shown)?;
    app.draw()?;
    app.run(|e| {
        if let AppEvent::Back = e {
            let mut m = model.borrow_mut();
            m.tab.back();
            show(&ui, &mut m);
        }
        describe(&app, &model.borrow(), &mut shown)?;
        Ok(true)
    })
}

/// Put the current page into the UI: the engine's page, the address bar, the footer.
fn show(ui: &BrowserApp, m: &mut Model) {
    let url = String::from(m.tab.current());
    let loaded = m.engine.load(&url);
    let header = Header::of(&url);
    let s = ui.global::<BrowserState>();
    s.set_page(loaded.page as i32);
    s.set_security(header.security as i32);
    s.set_domain(header.domain.as_str().into());
    s.set_can_back(m.tab.can_back());
    s.set_can_forward(m.tab.can_forward());
    s.set_count(count_label(1).as_str().into());
    let recents: Vec<SharedString> = start::recent(m.visited.iter().map(String::as_str))
        .into_iter()
        .map(SharedString::from)
        .collect();
    s.set_recents(ModelRc::from(Rc::new(VecModel::from(recents))));
}

/// The page for the shell: the title (the Open apps card) and Back staying in the app while there is history.
fn describe(app: &App, m: &Model, shown: &mut Page) -> Result<(), i32> {
    let mut title = FakeEngine.load(m.tab.current()).title;
    title.truncate(48);
    let page = Page {
        title,
        can_back: m.tab.can_back(),
        ..Page::default()
    };
    if page != *shown {
        app.page(&page)?;
        *shown = page;
    }
    Ok(())
}
