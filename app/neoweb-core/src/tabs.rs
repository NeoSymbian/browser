// SPDX-License-Identifier: Apache-2.0
//! Opened pages (browser-ui.md UI2, B03; WB5c): at most [`OPEN_MAX`] tabs, of which [`LOADED_MAX`] keep their page in
//! the engine; opening or showing one more evicts the least recently used (it reloads its address when shown
//! again). A page's new window is a tab with its opener, which Back returns to.

use alloc::string::String;
use alloc::vec::Vec;

pub const OPEN_MAX: usize = 30;
pub const LOADED_MAX: usize = 6;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tab {
    pub id: u32,
    pub url: String,
    pub title: String,
    /// The engine has its page.
    pub loaded: bool,
    /// The tab that opened it (a page's new window): Back from its first page goes there.
    pub opener: Option<u32>,
    /// When it was last shown (a counter).
    pub used: u64,
}

/// What the engine must do for a change of tabs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Change {
    /// The tab to show, and its address when its page must be made (new or evicted).
    pub show: Option<(u32, Option<String>)>,
    /// Pages to free (evicted, or their tab closed).
    pub free: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct Tabs {
    pub list: Vec<Tab>,
    pub current: Option<u32>,
    next_id: u32,
    clock: u64,
}

impl Tabs {
    pub fn get(&self, id: u32) -> Option<&Tab> {
        self.list.iter().find(|t| t.id == id)
    }

    fn get_mut(&mut self, id: u32) -> Option<&mut Tab> {
        self.list.iter_mut().find(|t| t.id == id)
    }

    pub fn current(&self) -> Option<&Tab> {
        self.current.and_then(|c| self.get(c))
    }

    pub fn full(&self) -> bool {
        self.list.len() >= OPEN_MAX
    }

    /// A new tab with `url`, shown; None when [`OPEN_MAX`] are open.
    pub fn open(&mut self, url: &str) -> Option<Change> {
        if self.full() {
            return None;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.list.push(Tab {
            id,
            url: url.into(),
            ..Tab::default()
        });
        Some(self.show(id))
    }

    /// The engine made tab `id` for a page of `opener`; it is shown already. Over [`OPEN_MAX`] the oldest tab goes.
    pub fn created(&mut self, id: u32, opener: u32) -> Change {
        let mut free = Vec::new();
        if self.full()
            && let Some(old) = self.lru(|_| true)
        {
            free.extend(self.close(old).free);
        }
        self.list.push(Tab {
            id,
            opener: Some(opener),
            loaded: true,
            ..Tab::default()
        });
        let mut c = self.show(id);
        c.show = None; // the engine shows it already
        c.free.extend(free);
        c
    }

    /// Show tab `id` (its page made again if it was evicted), evicting the least recently used over
    /// [`LOADED_MAX`].
    pub fn show(&mut self, id: u32) -> Change {
        self.clock += 1;
        let clock = self.clock;
        let Some(t) = self.get_mut(id) else {
            return Change::default();
        };
        let reload = (!t.loaded).then(|| t.url.clone());
        t.loaded = true;
        t.used = clock;
        self.current = Some(id);
        let mut free = Vec::new();
        while self.list.iter().filter(|t| t.loaded).count() > LOADED_MAX {
            let Some(old) = self.lru(|t| t.loaded && t.id != id) else {
                break;
            };
            self.get_mut(old).unwrap().loaded = false;
            free.push(old);
        }
        Change {
            show: Some((id, reload)),
            free,
        }
    }

    /// Close tab `id`; the most recently used other tab is shown (None: no tab is left).
    pub fn close(&mut self, id: u32) -> Change {
        let Some(i) = self.list.iter().position(|t| t.id == id) else {
            return Change::default();
        };
        let t = self.list.remove(i);
        let mut c = Change {
            show: None,
            free: if t.loaded {
                alloc::vec![id]
            } else {
                Vec::new()
            },
        };
        if self.current == Some(id) {
            self.current = None;
            if let Some(next) = self.lru_rev() {
                let s = self.show(next);
                c.show = s.show;
                c.free.extend(s.free);
            }
        }
        c
    }

    /// A tab's address or title changed.
    pub fn info(&mut self, id: u32, url: &str, title: &str) {
        if let Some(t) = self.get_mut(id) {
            if !url.is_empty() {
                t.url = url.into();
            }
            t.title = title.into();
        }
    }

    /// The least recently used tab passing `keep`.
    fn lru(&self, keep: impl Fn(&Tab) -> bool) -> Option<u32> {
        self.list
            .iter()
            .filter(|t| keep(t))
            .min_by_key(|t| t.used)
            .map(|t| t.id)
    }

    /// The most recently used tab.
    fn lru_rev(&self) -> Option<u32> {
        self.list.iter().max_by_key(|t| t.used).map(|t| t.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec;

    #[test]
    fn the_seventh_page_evicts_the_least_recently_used() {
        let mut tabs = Tabs::default();
        for i in 1..=6 {
            let c = tabs.open(&format!("https://site{i}.org/")).unwrap();
            assert!(c.free.is_empty());
        }
        tabs.show(1); // used again: tab 2 is the oldest now
        let c = tabs.open("https://site7.org/").unwrap();
        assert_eq!(c.show, Some((7, Some("https://site7.org/".into()))));
        assert_eq!(c.free, vec![2]);
        assert!(!tabs.get(2).unwrap().loaded);
        // showing it again reloads its address and evicts the next oldest
        let c = tabs.show(2);
        assert_eq!(c.show, Some((2, Some("https://site2.org/".into()))));
        assert_eq!(c.free, vec![3]);
        // a loaded tab is just shown
        assert_eq!(tabs.show(1).show, Some((1, None)));
    }

    #[test]
    fn thirty_tabs_at_most() {
        let mut tabs = Tabs::default();
        for _ in 0..OPEN_MAX {
            assert!(tabs.open("https://a.org/").is_some());
        }
        assert!(tabs.open("https://a.org/").is_none());
        // a page's new window replaces the oldest instead
        let c = tabs.created(0x8000_0001, 30);
        assert!(c.free.contains(&1) || tabs.get(1).is_none());
        assert_eq!(tabs.list.len(), OPEN_MAX);
        assert_eq!(tabs.current, Some(0x8000_0001));
    }

    #[test]
    fn closing_shows_the_last_used_and_frees_the_page() {
        let mut tabs = Tabs::default();
        tabs.open("https://a.org/");
        tabs.open("https://b.org/");
        tabs.open("https://c.org/");
        tabs.show(1);
        let c = tabs.close(1);
        assert_eq!(c.free, vec![1]);
        assert_eq!(c.show, Some((3, None)));
        assert_eq!(tabs.close(2).show, None); // not the current one
        assert_eq!(tabs.close(3).show, None);
        assert_eq!(tabs.current, None);
    }

    #[test]
    fn a_new_window_keeps_its_opener() {
        let mut tabs = Tabs::default();
        tabs.open("https://a.org/");
        let c = tabs.created(0x8000_0001, 1);
        assert_eq!(c.show, None);
        assert_eq!(tabs.current().unwrap().opener, Some(1));
        tabs.info(0x8000_0001, "https://b.org/x", "B");
        assert_eq!(tabs.get(0x8000_0001).unwrap().title, "B");
    }
}
