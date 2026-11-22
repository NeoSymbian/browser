// SPDX-License-Identifier: Apache-2.0
//! NEOWEBE's tabs (browser WB5c): one Servo WebView per loaded tab, one shown. NEOWEB numbers its tabs; a page's
//! new window gets an engine number (`neoweb_proto::ENGINE_TABS` and up). Only the shown tab paints; the others keep
//! running (their address and title go to NEOWEB as `TabInfo`).

use neoweb_proto::{Event, Load};
use servo::WebView;

/// What NEOWEB last heard of a tab, sent again when it is shown.
pub struct Tab {
    pub id: u32,
    pub view: WebView,
    pub url: String,
    pub title: String,
    pub back: bool,
    pub forward: bool,
    pub loading: bool,
    /// the error page it shows (UI5), until its next load
    pub error: Option<Event>,
}

#[derive(Default)]
pub struct Tabs {
    pub list: Vec<Tab>,
    pub shown: Option<u32>,
    next_engine: u32,
}

impl Tabs {
    pub fn add(&mut self, id: u32, view: WebView) {
        self.list.push(Tab {
            id,
            view,
            url: String::new(),
            title: String::new(),
            back: false,
            forward: false,
            loading: true,
            error: None,
        });
    }

    /// A number for a tab the engine makes (a page's new window).
    pub fn engine_id(&mut self) -> u32 {
        self.next_engine += 1;
        neoweb_proto::ENGINE_TABS + self.next_engine
    }

    pub fn get(&self, id: u32) -> Option<&Tab> {
        self.list.iter().find(|t| t.id == id)
    }

    /// The tab of `view`.
    pub fn of(&mut self, view: &WebView) -> Option<&mut Tab> {
        let id = view.id();
        self.list.iter_mut().find(|t| t.view.id() == id)
    }

    pub fn shown_view(&self) -> Option<&WebView> {
        self.shown.and_then(|s| self.get(s)).map(|t| &t.view)
    }

    /// Show tab `id`: the others are hidden (not painted). Returns the events that bring NEOWEB up to date.
    pub fn show(&mut self, id: u32) -> Vec<Event> {
        let Some(t) = self.get(id) else {
            return Vec::new();
        };
        for other in &self.list {
            if other.id != id {
                other.view.hide();
                other.view.set_focused(false);
            }
        }
        t.view.show();
        t.view.set_focused(true);
        // a new page has nothing to tell yet (an empty address would blank NEOWEB's)
        if t.url.is_empty() {
            self.shown = Some(id);
            return Vec::new();
        }
        let mut events = vec![
            Event::Url(t.url.clone()),
            Event::Title(t.title.clone()),
            Event::History {
                back: t.back,
                forward: t.forward,
            },
            Event::Load(if t.loading {
                Load::Started
            } else {
                Load::Complete
            }),
        ];
        events.extend(t.error.clone());
        self.shown = Some(id);
        events
    }

    /// Free tab `id`'s page.
    pub fn close(&mut self, id: u32) {
        self.list.retain(|t| t.id != id);
        if self.shown == Some(id) {
            self.shown = None;
        }
    }

    /// `event` about `view`'s page for NEOWEB: as is for the shown tab, else as TabInfo (address and title only).
    pub fn route(&mut self, view: &WebView, event: Event) -> Option<Event> {
        let shown = self.shown;
        let t = self.of(view)?;
        match &event {
            Event::Url(u) => t.url.clone_from(u),
            Event::Title(s) => t.title.clone_from(s),
            Event::History { back, forward } => (t.back, t.forward) = (*back, *forward),
            Event::Load(l) => {
                t.loading = *l != Load::Complete;
                if *l == Load::Started {
                    t.error = None;
                }
            }
            Event::PageError { .. } => t.error = Some(event.clone()),
            _ => {}
        }
        if shown == Some(t.id) {
            return Some(event);
        }
        matches!(event, Event::Url(_) | Event::Title(_)).then(|| Event::TabInfo {
            tab: t.id,
            url: t.url.clone(),
            title: t.title.clone(),
        })
    }
}
