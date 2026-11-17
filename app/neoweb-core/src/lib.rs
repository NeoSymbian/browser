// SPDX-License-Identifier: Apache-2.0
//! NEOWEB's logic, free of Slint and Symbian so it is tested on the host (docs `browser-ui.md`).
//!
//! - [`Tab`]: one opened page's history (← →, `can_back`);
//! - [`Header`] / [`count_label`]: what the address bar and the footer show (`br2-footer-states`);
//! - [`start`]: the start page's shortcuts (`br2-start`);
//! - [`Engine`] / [`FakeEngine`]: the page engine; the stand-in serves the boards' pages until Servo (WB4).
#![no_std]

extern crate alloc;

pub mod start;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The start page's address: the first entry of a new tab, so ← from a page comes back to it.
pub const START: &str = "neo:start";

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
}

/// What the address bar shows for a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub security: Security,
    /// the host only: the path is hidden (B01)
    pub domain: String,
}

impl Header {
    pub fn of(url: &str) -> Self {
        let (security, rest) = if let Some(r) = url.strip_prefix("https://") {
            (Security::Secure, r)
        } else if let Some(r) = url.strip_prefix("http://") {
            (Security::NotSecure, r)
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

/// The page engine behind NEOWEB's chrome: [`FakeEngine`] now, Servo from WB4 (browser-ui.md "Engine").
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
