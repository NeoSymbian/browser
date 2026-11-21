// SPDX-License-Identifier: Apache-2.0
//! NEOWEB's logic, free of Slint and Symbian so it is tested on the host (docs `browser-ui.md`).
//!
//! - [`Tab`]: one opened page's history (← →, `can_back`);
//! - [`LiveTab`]: the tab and chrome state reported by the engine process;
//! - [`Header`] / [`count_label`]: what the address bar and the footer show (`br2-footer-states`);
//! - [`start`]: the start page's shortcuts (`br2-start`);
//! - [`input`]: the address bar's text and suggestions (`br2-address`); [`chrome`]: hiding on scroll, loading;
//! - [`controls`]: the page's controls (Back order, script dialogs, uploads; WB5a);
//! - [`library`]: bookmarks, history, downloads and settings, and their saved text (UI4);
//! - [`Engine`] / [`FakeEngine`]: the stand-in used by the host tests.
#![no_std]

extern crate alloc;

pub mod chrome;
pub mod controls;
pub mod input;
pub mod library;
pub mod start;
pub mod tabs;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use neoweb_proto::Event;

/// The start page's address: the first entry of a new tab, so ← from a page comes back to it.
pub const START: &str = "neo:start";

/// The current engine page. Servo owns its history and tells the chrome which navigation is possible.
#[derive(Debug, Default)]
pub struct LiveTab {
    pub url: String,
    pub title: String,
    pub back: bool,
    pub forward: bool,
    pub loading: chrome::Loading,
    pub crashed: bool,
}

impl LiveTab {
    /// Apply a protocol event to the tab. Frame pixels and controls are handled by the UI process.
    pub fn apply(&mut self, event: Event) {
        if self.crashed {
            return;
        }
        match event {
            Event::Load(phase) => self.loading.update(phase),
            Event::Title(title) => self.title = title,
            Event::Url(url) => self.url = url,
            Event::History { back, forward } => {
                self.back = back;
                self.forward = forward;
            }
            Event::Crashed(_) => {
                self.crashed = true;
                self.loading.stop();
                self.back = false;
                self.forward = false;
            }
            // frames and the page's controls are NEOWEB's (symbian/neoweb)
            _ => {}
        }
    }
}

/// One opened page: its history and where in it the page is.
#[derive(Debug, Clone)]
pub struct Tab {
    history: Vec<String>,
    at: usize,
}

impl Default for Tab {
    fn default() -> Self {
        Self {
            history: alloc::vec![START.into()],
            at: 0,
        }
    }
}

impl Tab {
    pub fn current(&self) -> &str {
        &self.history[self.at]
    }
    /// Go to `url`: the forward history is dropped, as in every browser.
    pub fn open(&mut self, url: &str) {
        self.history.truncate(self.at + 1);
        self.history.push(url.into());
        self.at += 1;
    }
    pub fn can_back(&self) -> bool {
        self.at > 0
    }
    pub fn can_forward(&self) -> bool {
        self.at + 1 < self.history.len()
    }
    pub fn back(&mut self) -> bool {
        let ok = self.can_back();
        self.at -= usize::from(ok);
        ok
    }
    pub fn forward(&mut self) -> bool {
        let ok = self.can_forward();
        self.at += usize::from(ok);
        ok
    }
}

/// The security icon left of the domain (`br2-spec-chrome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// the start page and `neo:` pages: "Search or type an address"
    None = 0,
    /// https: g-lock
    Secure = 1,
    /// http: g-info + "Not secure"
    NotSecure = 2,
    /// a local file: g-info with the file address
    Local = 3,
}

/// What the address bar shows for a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub security: Security,
    /// The web host (B01), or the address of a local file.
    pub domain: String,
}

impl Header {
    pub fn of(url: &str) -> Self {
        let (security, rest) = if let Some(r) = url.strip_prefix("https://") {
            (Security::Secure, r)
        } else if let Some(r) = url.strip_prefix("http://") {
            (Security::NotSecure, r)
        } else if url.starts_with("file:") {
            return Self {
                security: Security::Local,
                domain: url.into(),
            };
        } else {
            return Self {
                security: Security::None,
                domain: String::new(),
            };
        };
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        Self {
            security,
            domain: host.to_string(),
        }
    }
}

/// The count drawn inside the footer's Opened pages icon: "9+" past nine (`br2-footer-states`).
pub fn count_label(pages: usize) -> String {
    if pages > 9 {
        "9+".into()
    } else {
        pages.to_string()
    }
}

/// A page the engine has loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub title: String,
    /// which stand-in page the UI draws ([`FakePage`]); the real engine draws pixels instead (WB4)
    pub page: FakePage,
}

/// A stand-in page engine for the host tests (browser-ui.md "Engine").
pub trait Engine {
    fn load(&mut self, url: &str) -> Loaded;
}

/// The stand-in pages, drawn in Slint (neoweb-ui `FakePages`), from the canvas boards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakePage {
    Start = 0,
    /// en.wikipedia.org/wiki/Symbian (`br2-page`)
    Wikipedia = 1,
    /// github.com (`br2-login-save`'s dashboard)
    GitHub = 2,
    /// any other address: a plain page naming it
    Other = 3,
}

/// The stand-in engine (B08): every screen works before Servo draws on the phone.
#[derive(Debug, Default)]
pub struct FakeEngine;

impl Engine for FakeEngine {
    fn load(&mut self, url: &str) -> Loaded {
        let host = Header::of(url).domain;
        let (title, page) = match host.as_str() {
            _ if url == START => ("New tab".into(), FakePage::Start),
            "en.wikipedia.org" => ("Symbian – Wikipedia".into(), FakePage::Wikipedia),
            "github.com" => ("NeoSymbian · GitHub".into(), FakePage::GitHub),
            _ => (host, FakePage::Other),
        };
        Loaded { title, page }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoweb_proto::Load;

    #[test]
    fn live_load_follows_engine_milestones() {
        let mut tab = LiveTab::default();
        tab.apply(Event::Load(Load::Started));
        let start = tab.loading.progress();
        assert!(tab.loading.active() && start > 0.0);
        tab.apply(Event::Load(Load::HeadParsed));
        assert!(
            tab.loading.active() && start < tab.loading.progress() && tab.loading.progress() < 1.0
        );
        tab.apply(Event::Load(Load::Complete));
        assert!(!tab.loading.active());
        assert_eq!(tab.loading.progress(), 1.0);
    }

    #[test]
    fn live_tab_uses_reported_title_url_and_history() {
        let mut tab = LiveTab::default();
        tab.apply(Event::Url("https://example.org/redirect".into()));
        tab.apply(Event::Url("https://example.net/final".into()));
        tab.apply(Event::Title("A page · էջ".into()));
        assert_eq!(tab.title, "A page · էջ");
        assert_eq!(tab.url, "https://example.net/final");
        assert_eq!(Header::of(&tab.url).domain, "example.net");
        for (back, forward) in [(true, false), (false, true), (false, false)] {
            tab.apply(Event::History { back, forward });
            assert_eq!((tab.back, tab.forward), (back, forward));
        }
    }

    #[test]
    fn live_crash_keeps_the_reload_url_and_ends_loading() {
        let mut tab = LiveTab::default();
        tab.apply(Event::Url("file:///c:/pages/a.html".into()));
        tab.apply(Event::Load(Load::Started));
        tab.apply(Event::History {
            back: true,
            forward: true,
        });
        tab.apply(Event::Crashed("engine terminated".into()));
        tab.apply(Event::Load(Load::HeadParsed));
        assert!(tab.crashed && !tab.loading.active() && !tab.back && !tab.forward);
        assert_eq!(tab.url, "file:///c:/pages/a.html");
        assert_eq!(Header::of(&tab.url).security, Security::Local);
    }

    #[test]
    fn history_walks_back_and_forward() {
        let mut t = Tab::default();
        assert_eq!(t.current(), START);
        assert!(!t.can_back() && !t.can_forward());
        t.open("https://en.wikipedia.org/wiki/Symbian");
        t.open("https://github.com/NeoSymbian");
        assert!(t.back());
        assert_eq!(t.current(), "https://en.wikipedia.org/wiki/Symbian");
        assert!(t.can_forward());
        // a new address drops the forward history
        t.open("http://symbian.example.org/");
        assert!(!t.can_forward());
        assert!(t.back() && t.back());
        assert_eq!(t.current(), START);
        assert!(
            !t.back(),
            "the first page: Back leaves the app (the shell's)"
        );
    }

    #[test]
    fn header_shows_the_host_and_security() {
        let h = Header::of("https://en.wikipedia.org/wiki/Symbian?x=1");
        assert_eq!(
            (h.security, h.domain.as_str()),
            (Security::Secure, "en.wikipedia.org")
        );
        assert_eq!(
            Header::of("http://symbian.example.org").security,
            Security::NotSecure
        );
        assert_eq!(
            Header::of(START),
            Header {
                security: Security::None,
                domain: String::new()
            }
        );
    }

    #[test]
    fn count_tops_out_at_nine_plus() {
        assert_eq!(count_label(1), "1");
        assert_eq!(count_label(9), "9");
        assert_eq!(count_label(10), "9+");
    }

    #[test]
    fn fake_engine_knows_the_boards_pages() {
        let mut e = FakeEngine;
        assert_eq!(
            e.load("https://en.wikipedia.org/wiki/Symbian").page,
            FakePage::Wikipedia
        );
        assert_eq!(
            e.load("https://github.com/NeoSymbian").page,
            FakePage::GitHub
        );
        let other = e.load("https://slint.dev/docs");
        assert_eq!(
            (other.page, other.title.as_str()),
            (FakePage::Other, "slint.dev")
        );
        assert_eq!(e.load(START).page, FakePage::Start);
    }
}
