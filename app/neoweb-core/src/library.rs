// SPDX-License-Identifier: Apache-2.0
//! The library and settings (browser-ui.md UI4): bookmarks with one level of folders (`br2-bookmarks`), history
//! grouped by day (`br2-history`), the download list (`br2-downloads`) and the settings (`br2-settings`).
//!
//! NEOWEB keeps each model in its private folder as line-based UTF-8 text: one record per line, fields separated
//! by tabs, with `\\`, `\t`, `\n` and `\r` escaped inside a field. Unknown or bad lines are skipped.
//! Times are seconds since 1970 in local time (the phone's clock), so a day starts at a multiple of 86 400.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---- the text format ----

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(c) => out.push(c),
            None => {}
        }
    }
    out
}

/// One record: its fields joined by tabs, escaped, and a newline.
fn line(out: &mut String, fields: &[&str]) {
    let escaped: Vec<String> = fields.iter().map(|f| escape(f)).collect();
    out.push_str(&escaped.join("\t"));
    out.push('\n');
}

/// The records of a text: each line's fields, unescaped.
fn records(text: &str) -> impl Iterator<Item = Vec<String>> + '_ {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.split('\t').map(unescape).collect())
}

// ---- time labels ----

const DAY: i64 = 86_400;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// (year, month 1..=12, day) of a day number since 1970 (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// "HH:MM" of a time.
pub fn clock(time: i64) -> String {
    let s = time.rem_euclid(DAY);
    format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
}

/// The day of `time` as seen at `now`: "Today", "Yesterday", "18 Nov", or "18 Nov 2025" in another year.
pub fn day_label(time: i64, now: i64) -> String {
    let (day, today) = (time.div_euclid(DAY), now.div_euclid(DAY));
    if day == today {
        return "Today".into();
    }
    if day == today - 1 {
        return "Yesterday".into();
    }
    let (y, m, d) = civil(day);
    let month = MONTHS[(m - 1) as usize];
    if y == civil(today).0 {
        format!("{d} {month}")
    } else {
        format!("{d} {month} {y}")
    }
}

/// The host of an address ("en.wikipedia.org"), or the address itself when it has none.
pub fn domain(url: &str) -> String {
    let h = crate::Header::of(url).domain;
    if h.is_empty() { url.into() } else { h }
}

// ---- bookmarks ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    /// The folder it is in; None = the top level.
    pub folder: Option<u32>,
}

/// Bookmarks and their folders (one level), each in the order they were added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Bookmarks {
    pub folders: Vec<Folder>,
    pub list: Vec<Bookmark>,
}

impl Bookmarks {
    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.list.iter().any(|b| b.url == url)
    }

    /// ☆ in the address bar: a bookmark at the top level; false when the address has one already.
    pub fn add(&mut self, title: &str, url: &str) -> bool {
        if self.is_bookmarked(url) {
            return false;
        }
        self.list.push(Bookmark {
            title: title.into(),
            url: url.into(),
            folder: None,
        });
        true
    }

    /// ☆ again: the address's bookmark goes.
    pub fn remove_url(&mut self, url: &str) {
        self.list.retain(|b| b.url != url);
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.list.len() {
            self.list.remove(i);
        }
    }

    pub fn edit(&mut self, i: usize, title: &str, url: &str) {
        if let Some(b) = self.list.get_mut(i) {
            b.title = title.into();
            b.url = url.into();
        }
    }

    /// Move bookmark `i` into `folder` (None: the top level); an unknown folder is ignored.
    pub fn move_to(&mut self, i: usize, folder: Option<u32>) {
        if folder.is_some_and(|f| self.folder(f).is_none()) {
            return;
        }
        if let Some(b) = self.list.get_mut(i) {
            b.folder = folder;
        }
    }

    pub fn folder(&self, id: u32) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// A new folder; its id.
    pub fn add_folder(&mut self, name: &str) -> u32 {
        let id = self.folders.iter().map(|f| f.id).max().unwrap_or(0) + 1;
        self.folders.push(Folder {
            id,
            name: name.into(),
        });
        id
    }

    pub fn rename_folder(&mut self, id: u32, name: &str) {
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.name = name.into();
        }
    }

    /// Delete a folder; its bookmarks move to the top level.
    pub fn remove_folder(&mut self, id: u32) {
        self.folders.retain(|f| f.id != id);
        for b in self.list.iter_mut().filter(|b| b.folder == Some(id)) {
            b.folder = None;
        }
    }

    /// The bookmarks in `folder` (None: the top level) with their indices in `list`.
    pub fn in_folder(&self, folder: Option<u32>) -> impl Iterator<Item = (usize, &Bookmark)> {
        self.list
            .iter()
            .enumerate()
            .filter(move |(_, b)| b.folder == folder)
    }

    /// A folder row's detail: "6 bookmarks".
    pub fn count_label(&self, folder: u32) -> String {
        match self.in_folder(Some(folder)).count() {
            1 => "1 bookmark".into(),
            n => format!("{n} bookmarks"),
        }
    }

    /// `F id name` and `B folder title url` (folder empty at the top level).
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for f in &self.folders {
            line(&mut out, &["F", &f.id.to_string(), &f.name]);
        }
        for b in &self.list {
            let folder = b.folder.map(|f| f.to_string()).unwrap_or_default();
            line(&mut out, &["B", &folder, &b.title, &b.url]);
        }
        out
    }

    pub fn decode(text: &str) -> Self {
        let mut m = Self::default();
        for r in records(text) {
            match r.as_slice() {
                [k, id, name] if k == "F" => {
                    if let Ok(id) = id.parse()
                        && m.folder(id).is_none()
                    {
                        m.folders.push(Folder {
                            id,
                            name: name.clone(),
                        });
                    }
                }
                [k, folder, title, url] if k == "B" && !url.is_empty() => m.list.push(Bookmark {
                    title: title.clone(),
                    url: url.clone(),
                    folder: folder.parse().ok(),
                }),
                _ => {}
            }
        }
        // a bookmark in a folder that is gone is at the top level
        for i in 0..m.list.len() {
            if m.list[i].folder.is_some_and(|f| m.folder(f).is_none()) {
                m.list[i].folder = None;
            }
        }
        m
    }
}

// ---- history ----

pub const HISTORY_MAX: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub title: String,
    pub url: String,
    pub time: i64,
}

/// Visited pages, oldest first; one entry per address (its last visit), at most [`HISTORY_MAX`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct History {
    pub list: Vec<Visit>,
}

/// A row of `br2-history`: a day's header, or a visit (its index in `History::list` and "domain · HH:MM").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryRow {
    Day(String),
    Visit { index: usize, detail: String },
}

impl History {
    /// A visit: the address moves to the newest place with this title and time.
    pub fn add(&mut self, title: &str, url: &str, time: i64) {
        self.list.retain(|v| v.url != url);
        self.list.push(Visit {
            title: title.into(),
            url: url.into(),
            time,
        });
        if self.list.len() > HISTORY_MAX {
            self.list.drain(..self.list.len() - HISTORY_MAX);
        }
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.list.len() {
            self.list.remove(i);
        }
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// The (title, address) pairs `input::suggest` takes, oldest first.
    pub fn pairs(&self) -> Vec<(String, String)> {
        self.list
            .iter()
            .map(|v| (v.title.clone(), v.url.clone()))
            .collect()
    }

    /// The rows newest first, a day's header before its visits; `filter` (any case) keeps the visits whose title or
    /// address contains it ("" = all).
    pub fn rows(&self, now: i64, filter: &str) -> Vec<HistoryRow> {
        let f = filter.trim().to_lowercase();
        let mut order: Vec<usize> = (0..self.list.len())
            .filter(|&i| {
                let v = &self.list[i];
                f.is_empty()
                    || v.title.to_lowercase().contains(&f)
                    || v.url.to_lowercase().contains(&f)
            })
            .collect();
        // newest first; a stable sort keeps the later-added first among equal times
        order.reverse();
        order.sort_by(|&a, &b| self.list[b].time.cmp(&self.list[a].time));
        let mut out = Vec::new();
        let mut day = None;
        for i in order {
            let v = &self.list[i];
            let d = v.time.div_euclid(DAY);
            if day != Some(d) {
                day = Some(d);
                out.push(HistoryRow::Day(day_label(v.time, now)));
            }
            out.push(HistoryRow::Visit {
                index: i,
                detail: format!("{} · {}", domain(&v.url), clock(v.time)),
            });
        }
        out
    }

    /// `V time title url`, oldest first.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for v in &self.list {
            line(&mut out, &["V", &v.time.to_string(), &v.title, &v.url]);
        }
        out
    }

    pub fn decode(text: &str) -> Self {
        let mut m = Self::default();
        for r in records(text) {
            if let [k, time, title, url] = r.as_slice()
                && k == "V"
                && !url.is_empty()
                && let Ok(time) = time.parse()
            {
                m.add(title, url, time);
            }
        }
        m
    }
}

// ---- downloads ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    Downloading = 0,
    Paused = 1,
    Done = 2,
    Failed = 3,
}

impl DownloadState {
    fn from_index(i: u8) -> Option<Self> {
        [Self::Downloading, Self::Paused, Self::Done, Self::Failed]
            .get(usize::from(i))
            .copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub id: u32,
    /// The file's name ("EKA2-internals.pdf").
    pub name: String,
    pub url: String,
    /// Where it is saved (`C:\Data\Downloads\…`).
    pub path: String,
    /// Bytes in all; 0 = not known.
    pub size: u64,
    pub received: u64,
    pub state: DownloadState,
    /// When it finished (Done).
    pub finished: i64,
}

/// The icon of a finished download in `br2-downloads`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Doc = 0,
    Image = 1,
    File = 2,
}

/// "860 KB", "2.4 MB" (1 KB = 1024 bytes).
pub fn size_text(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * KB;
    const GB: u64 = MB * KB;
    match bytes {
        b if b < KB => format!("{b} B"),
        b if b < MB => format!("{} KB", (b + KB / 2) / KB),
        b if b < GB => tenths(b, MB, "MB"),
        b => tenths(b, GB, "GB"),
    }
}

/// `b / unit` with one decimal, rounded: "8.4 MB".
fn tenths(b: u64, unit: u64, name: &str) -> String {
    let t = (b * 10 + unit / 2) / unit;
    format!("{}.{} {name}", t / 10, t % 10)
}

impl Download {
    pub fn kind(&self) -> FileKind {
        let ext = self
            .name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" => FileKind::Image,
            "pdf" | "txt" | "doc" | "docx" | "odt" | "rtf" | "html" | "htm" | "md" | "epub" => {
                FileKind::Doc
            }
            _ => FileKind::File,
        }
    }

    /// 0..=1 for the progress bar (0 while the size is unknown).
    pub fn fraction(&self) -> f32 {
        if self.size == 0 {
            0.0
        } else {
            (self.received.min(self.size) as f32) / (self.size as f32)
        }
    }

    /// A row's line under the name: "3.2 of 8.4 MB · 13 s left" (time left from `rate`, bytes per second; 0 =
    /// unknown), "Paused · 3.2 of 8.4 MB", "Failed", or when done "2.4 MB · Today 09:52" / "1.9 MB · Yesterday".
    pub fn detail(&self, rate: u64, now: i64) -> String {
        match self.state {
            DownloadState::Done => {
                let day = day_label(self.finished, now);
                if day == "Today" {
                    format!("{} · Today {}", size_text(self.size), clock(self.finished))
                } else {
                    format!("{} · {day}", size_text(self.size))
                }
            }
            DownloadState::Failed => "Failed".into(),
            DownloadState::Paused => format!("Paused · {}", self.amount()),
            DownloadState::Downloading => {
                let left = self.size.saturating_sub(self.received);
                if rate == 0 || self.size == 0 {
                    self.amount()
                } else {
                    format!("{} · {} left", self.amount(), duration(left.div_ceil(rate)))
                }
            }
        }
    }

    /// "3.2 of 8.4 MB", in the size's unit; "3.2 MB" while the size is unknown.
    fn amount(&self) -> String {
        if self.size == 0 {
            return size_text(self.received);
        }
        let total = size_text(self.size);
        let (_, unit) = total.split_once(' ').unwrap_or(("", "B"));
        let scale = match unit {
            "KB" => 1024,
            "MB" => 1024 * 1024,
            "GB" => 1024 * 1024 * 1024,
            _ => 1,
        };
        let got = match unit {
            "B" => self.received.to_string(),
            "KB" => ((self.received + scale / 2) / scale).to_string(),
            _ => tenths(self.received, scale, "").trim_end().to_string(),
        };
        format!("{got} of {total}")
    }
}

/// "13 s", "4 min", "2 h".
fn duration(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", s.div_ceil(60)),
        s => format!("{} h", s.div_ceil(3600)),
    }
}

/// The download list, oldest first.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Downloads {
    pub list: Vec<Download>,
}

impl Downloads {
    /// A new download (Downloading); its id.
    pub fn add(&mut self, name: &str, url: &str, path: &str, size: u64) -> u32 {
        let id = self.list.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        self.list.push(Download {
            id,
            name: name.into(),
            url: url.into(),
            path: path.into(),
            size,
            received: 0,
            state: DownloadState::Downloading,
            finished: 0,
        });
        id
    }

    pub fn get(&self, id: u32) -> Option<&Download> {
        self.list.iter().find(|d| d.id == id)
    }

    fn get_mut(&mut self, id: u32) -> Option<&mut Download> {
        self.list.iter_mut().find(|d| d.id == id)
    }

    pub fn progress(&mut self, id: u32, received: u64) {
        if let Some(d) = self.get_mut(id) {
            d.received = received;
        }
    }

    pub fn finish(&mut self, id: u32, time: i64) {
        if let Some(d) = self.get_mut(id) {
            d.state = DownloadState::Done;
            d.finished = time;
            d.size = d.size.max(d.received);
            d.received = d.size;
        }
    }

    pub fn fail(&mut self, id: u32) {
        if let Some(d) = self.get_mut(id) {
            d.state = DownloadState::Failed;
        }
    }

    /// Pause ⇄ resume; the new state.
    pub fn toggle_pause(&mut self, id: u32) -> Option<DownloadState> {
        let d = self.get_mut(id)?;
        d.state = match d.state {
            DownloadState::Downloading => DownloadState::Paused,
            DownloadState::Paused => DownloadState::Downloading,
            s => s,
        };
        Some(d.state)
    }

    /// Cancel a download, or drop a finished one from the list.
    pub fn remove(&mut self, id: u32) {
        self.list.retain(|d| d.id != id);
    }

    /// The `br2-downloads` sections: the ones still going (Downloading, Paused, Failed), then Done; newest first.
    pub fn sections(&self) -> (Vec<&Download>, Vec<&Download>) {
        let (done, going): (Vec<&Download>, Vec<&Download>) = self
            .list
            .iter()
            .rev()
            .partition(|d| d.state == DownloadState::Done);
        (going, done)
    }

    /// `D id state size received finished name url path`. A download that was going comes back Paused.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for d in &self.list {
            line(
                &mut out,
                &[
                    "D",
                    &d.id.to_string(),
                    &(d.state as u8).to_string(),
                    &d.size.to_string(),
                    &d.received.to_string(),
                    &d.finished.to_string(),
                    &d.name,
                    &d.url,
                    &d.path,
                ],
            );
        }
        out
    }

    pub fn decode(text: &str) -> Self {
        let mut m = Self::default();
        for r in records(text) {
            let [k, id, state, size, received, finished, name, url, path] = r.as_slice() else {
                continue;
            };
            let (Ok(id), Some(mut state), Ok(size), Ok(received), Ok(finished)) = (
                id.parse(),
                state.parse().ok().and_then(DownloadState::from_index),
                size.parse(),
                received.parse(),
                finished.parse(),
            ) else {
                continue;
            };
            if k != "D" || m.get(id).is_some() {
                continue;
            }
            if state == DownloadState::Downloading {
                state = DownloadState::Paused;
            }
            m.list.push(Download {
                id,
                name: name.clone(),
                url: url.clone(),
                path: path.clone(),
                size,
                received,
                state,
                finished,
            });
        }
        m
    }
}

// ---- settings ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchEngine {
    #[default]
    DuckDuckGo = 0,
    Google = 1,
    Bing = 2,
    Startpage = 3,
}

impl SearchEngine {
    pub const ALL: [Self; 4] = [Self::DuckDuckGo, Self::Google, Self::Bing, Self::Startpage];

    /// The engine at a ChoiceSheet index (`set-engine` i).
    pub fn from_index(i: i32) -> Option<Self> {
        usize::try_from(i)
            .ok()
            .and_then(|i| Self::ALL.get(i))
            .copied()
    }

    pub fn name(self) -> &'static str {
        ["DuckDuckGo", "Google", "Bing", "Startpage"][self as usize]
    }

    /// The address of a web search for `text`.
    pub fn search(self, text: &str) -> String {
        let base = [
            "https://duckduckgo.com/?q=",
            "https://www.google.com/search?q=",
            "https://www.bing.com/search?q=",
            "https://www.startpage.com/do/search?q=",
        ][self as usize];
        format!("{base}{}", crate::input::encode(text.trim()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cookies {
    AllowAll = 0,
    #[default]
    BlockThirdParty = 1,
}

/// The Text size choices (percent); `set-text-size` i picks one.
pub const TEXT_SIZES: [u8; 6] = [80, 90, 100, 115, 130, 150];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub engine: SearchEngine,
    /// Percent, 80..=150.
    pub text_size: u8,
    pub desktop_site: bool,
    pub javascript: bool,
    pub cookies: Cookies,
    /// B10: certificate errors offer "Open the site anyway (unsafe)".
    pub allow_unsafe_certs: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            engine: SearchEngine::DuckDuckGo,
            text_size: 100,
            desktop_site: false,
            javascript: true,
            cookies: Cookies::BlockThirdParty,
            allow_unsafe_certs: false,
        }
    }
}

impl Settings {
    pub fn set_text_size(&mut self, percent: i64) {
        self.text_size = percent.clamp(80, 150) as u8;
    }

    /// `key value` lines.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        let b = |v: bool| if v { "1" } else { "0" };
        line(&mut out, &["engine", self.engine.name()]);
        line(&mut out, &["text-size", &self.text_size.to_string()]);
        line(&mut out, &["desktop-site", b(self.desktop_site)]);
        line(&mut out, &["javascript", b(self.javascript)]);
        let cookies = match self.cookies {
            Cookies::AllowAll => "allow-all",
            Cookies::BlockThirdParty => "block-third-party",
        };
        line(&mut out, &["cookies", cookies]);
        line(
            &mut out,
            &["allow-unsafe-certs", b(self.allow_unsafe_certs)],
        );
        out
    }

    /// The defaults with each known, valid line applied.
    pub fn decode(text: &str) -> Self {
        let mut s = Self::default();
        for r in records(text) {
            let [k, v] = r.as_slice() else { continue };
            let flag = match v.as_str() {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            };
            match (k.as_str(), flag) {
                ("engine", _) => {
                    if let Some(e) = SearchEngine::ALL.iter().find(|e| e.name() == v) {
                        s.engine = *e;
                    }
                }
                ("text-size", _) => {
                    if let Ok(p) = v.parse() {
                        s.set_text_size(p);
                    }
                }
                ("cookies", _) if v == "allow-all" => s.cookies = Cookies::AllowAll,
                ("cookies", _) if v == "block-third-party" => s.cookies = Cookies::BlockThirdParty,
                ("desktop-site", Some(f)) => s.desktop_site = f,
                ("javascript", Some(f)) => s.javascript = f,
                ("allow-unsafe-certs", Some(f)) => s.allow_unsafe_certs = f,
                _ => {}
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-20 10:24 (a Friday), local
    const NOW: i64 = 1_795_132_800 + 10 * 3600 + 24 * 60;

    #[test]
    fn now_is_the_twentieth() {
        assert_eq!(civil(NOW.div_euclid(DAY)), (2026, 11, 20));
        assert_eq!(clock(NOW), "10:24");
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(-1), (1969, 12, 31));
        assert_eq!(civil(11_016), (2000, 2, 29));
    }

    #[test]
    fn escaping_round_trips() {
        let s = "a\tb\nc\\d\re\\t";
        assert_eq!(unescape(&escape(s)), s);
        assert!(!escape(s).contains(['\t', '\n']));
    }

    #[test]
    fn bookmarks_with_folders() {
        let mut b = Bookmarks::default();
        assert!(b.add(
            "Symbian — Wikipedia",
            "https://en.wikipedia.org/wiki/Symbian"
        ));
        assert!(!b.add("again", "https://en.wikipedia.org/wiki/Symbian"));
        assert!(b.add("Servo", "https://servo.org/"));
        let docs = b.add_folder("Symbian docs");
        let read = b.add_folder("Reading list");
        assert_ne!(docs, read);
        b.move_to(0, Some(docs));
        b.move_to(1, Some(99)); // no such folder
        assert_eq!(b.list[1].folder, None);
        assert_eq!(b.count_label(docs), "1 bookmark");
        assert_eq!(b.count_label(read), "0 bookmarks");
        assert_eq!(b.in_folder(None).map(|(i, _)| i).collect::<Vec<_>>(), [1]);
        b.rename_folder(docs, "Docs");
        assert_eq!(b.folder(docs).unwrap().name, "Docs");
        assert!(b.is_bookmarked("https://servo.org/"));
        b.remove_url("https://servo.org/");
        assert!(!b.is_bookmarked("https://servo.org/"));
        b.remove_folder(docs);
        assert_eq!(b.list[0].folder, None);
        b.edit(0, "W", "https://w.org/");
        assert_eq!(b.list[0].url, "https://w.org/");
        b.remove(0);
        b.remove(5);
        assert!(b.list.is_empty());
    }

    #[test]
    fn bookmarks_round_trip_and_skip_bad_lines() {
        let mut b = Bookmarks::default();
        let f = b.add_folder("Tabs\tand\nnewlines \\ here");
        b.add("NeoSymbian", "https://github.com/NeoSymbian");
        b.add("Slint", "https://slint.dev/docs");
        b.move_to(1, Some(f));
        let text = b.encode();
        assert_eq!(Bookmarks::decode(&text), b);
        let noisy =
            format!("junk\nF\tx\tbad id\nB\t\tno url\t\n{text}B\t7\tlost\thttps://l.org/\n\u{0}\n");
        let d = Bookmarks::decode(&noisy);
        assert_eq!(d.folders, b.folders);
        assert_eq!(d.list.len(), 3);
        assert_eq!(d.list[2].folder, None); // its folder 7 is gone
    }

    #[test]
    fn history_groups_by_day_newest_first() {
        let mut h = History::default();
        let yesterday = NOW - DAY;
        h.add("Servo", "https://servo.org/", yesterday - 4 * 3600);
        h.add(
            "EKA2",
            "https://symbian.example.org/wiki/EKA2",
            yesterday + 11 * 3600,
        );
        h.add("Old", "https://old.org/", NOW - 2 * DAY);
        h.add("NeoSymbian", "https://github.com/NeoSymbian", NOW - 19 * 60);
        h.add(
            "Symbian",
            "https://en.wikipedia.org/wiki/Symbian",
            NOW - 3 * 60,
        );
        // the same address again moves it and updates its time
        h.add("Servo again", "https://servo.org/", NOW - 60);
        assert_eq!(h.list.len(), 5);
        let rows = h.rows(NOW, "");
        let v = |index: usize, detail: &str| HistoryRow::Visit {
            index,
            detail: detail.into(),
        };
        assert_eq!(
            rows,
            [
                HistoryRow::Day("Today".into()),
                v(4, "servo.org · 10:23"),
                v(3, "en.wikipedia.org · 10:21"),
                v(2, "github.com · 10:05"),
                HistoryRow::Day("Yesterday".into()),
                v(0, "symbian.example.org · 21:24"),
                HistoryRow::Day("18 Nov".into()),
                v(1, "old.org · 10:24"),
            ]
        );
        assert_eq!(h.rows(NOW, "GITHUB").len(), 2);
        assert_eq!(day_label(NOW - 400 * DAY, NOW), "16 Oct 2025");
        h.remove(4);
        assert_eq!(h.list.len(), 4);
        assert_eq!(h.pairs()[0].0, "EKA2");
        h.clear();
        assert!(h.rows(NOW, "").is_empty());
    }

    #[test]
    fn history_keeps_the_last_thousand_and_round_trips() {
        let mut h = History::default();
        for i in 0..1005 {
            h.add("t", &format!("https://s{i}.org/"), i);
        }
        assert_eq!(h.list.len(), HISTORY_MAX);
        assert_eq!(h.list[0].url, "https://s5.org/");
        let mut small = History::default();
        small.add("A\ttab", "https://a.org/", NOW);
        small.add("B", "https://b.org/?q=1\n", -5);
        let text = small.encode();
        assert_eq!(History::decode(&text), small);
        assert_eq!(
            History::decode(&format!("V\tnope\tx\thttps://x/\n{text}V\t1\n")),
            small
        );
    }

    #[test]
    fn download_texts() {
        let mut d = Downloads::default();
        let mb = 1024 * 1024;
        let id = d.add(
            "symbian3-dev-guide.pdf",
            "https://example.org/guide.pdf",
            "C:\\Data\\Downloads\\symbian3-dev-guide.pdf",
            (8.4 * mb as f64) as u64,
        );
        d.progress(id, (3.2 * mb as f64) as u64);
        let g = d.get(id).unwrap();
        assert_eq!(g.detail(420 * 1024, NOW), "3.2 of 8.4 MB · 13 s left");
        assert_eq!(g.detail(0, NOW), "3.2 of 8.4 MB");
        assert!((g.fraction() - 0.381).abs() < 0.01);
        assert_eq!(g.kind(), FileKind::Doc);
        assert_eq!(d.toggle_pause(id), Some(DownloadState::Paused));
        assert_eq!(d.get(id).unwrap().detail(0, NOW), "Paused · 3.2 of 8.4 MB");
        assert_eq!(d.toggle_pause(id), Some(DownloadState::Downloading));

        let img = d.add(
            "n8-wallpaper.jpg",
            "https://e.org/w.jpg",
            "C:\\w.jpg",
            860 * 1024,
        );
        d.finish(img, NOW - 54 * 60);
        assert_eq!(d.get(img).unwrap().detail(0, NOW), "860 KB · Today 09:30");
        assert_eq!(d.get(img).unwrap().kind(), FileKind::Image);
        let sis = d.add("frozen-bubble.sis", "https://e.org/f.sis", "C:\\f.sis", 0);
        d.progress(sis, (1.9 * mb as f64) as u64);
        assert_eq!(d.get(sis).unwrap().detail(1000, NOW), "1.9 MB");
        d.finish(sis, NOW - DAY);
        assert_eq!(d.get(sis).unwrap().detail(0, NOW), "1.9 MB · Yesterday");
        assert_eq!(d.get(sis).unwrap().kind(), FileKind::File);

        let (going, done) = d.sections();
        assert_eq!(going.iter().map(|d| d.id).collect::<Vec<_>>(), [id]);
        assert_eq!(done.iter().map(|d| d.id).collect::<Vec<_>>(), [sis, img]);
        d.fail(id);
        assert_eq!(d.get(id).unwrap().detail(0, NOW), "Failed");
        d.remove(id);
        assert!(d.get(id).is_none());

        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(5 * 1024 * mb / 2), "2.5 GB");
        assert_eq!(duration(61), "2 min");
        assert_eq!(duration(3601), "2 h");
    }

    #[test]
    fn downloads_round_trip_and_resume_paused() {
        let mut d = Downloads::default();
        let a = d.add("a\tb.pdf", "https://a.org/a", "C:\\a", 100);
        let b = d.add("c.jpg", "https://a.org/c", "C:\\c", 50);
        d.progress(a, 40);
        d.finish(b, NOW);
        let back = Downloads::decode(&d.encode());
        assert_eq!(back.get(a).unwrap().state, DownloadState::Paused);
        assert_eq!(back.get(b), d.get(b));
        let mut expect = d.clone();
        expect.toggle_pause(a);
        assert_eq!(back, expect);
        let noisy = format!("D\t1\t9\t0\t0\t0\tx\ty\tz\nD\tq\n{}", d.encode());
        assert_eq!(Downloads::decode(&noisy), expect);
    }

    #[test]
    fn settings_defaults_round_trip_and_bad_values() {
        let s = Settings::default();
        assert_eq!(s.engine, SearchEngine::DuckDuckGo);
        assert!(s.javascript && !s.allow_unsafe_certs && !s.desktop_site);
        assert_eq!(Settings::decode(""), s);
        let t = Settings {
            engine: SearchEngine::Startpage,
            text_size: 130,
            desktop_site: true,
            javascript: false,
            cookies: Cookies::AllowAll,
            allow_unsafe_certs: true,
        };
        assert_eq!(Settings::decode(&t.encode()), t);
        let bad = "engine\tAltaVista\ntext-size\t400\njavascript\tyes\ncolour\tred\nlonely\n";
        let d = Settings::decode(bad);
        assert_eq!(d.text_size, 150);
        assert_eq!(d.engine, SearchEngine::DuckDuckGo);
        assert!(d.javascript);
        assert_eq!(SearchEngine::from_index(1), Some(SearchEngine::Google));
        assert_eq!(SearchEngine::from_index(-1), None);
        assert_eq!(
            SearchEngine::Google.search("eka2 kernel"),
            "https://www.google.com/search?q=eka2+kernel"
        );
    }
}
