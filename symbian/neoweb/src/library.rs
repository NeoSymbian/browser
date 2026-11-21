// SPDX-License-Identifier: GPL-3.0-only
//! NEOWEB's library (browser-ui.md UI4, WB5d): bookmarks, history, downloads and settings (neoweb-core's models),
//! kept in NEOWEB's private folder, shown in the library views, and the downloads' shade entries.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use e32::fs::{File, Fs, mode};
use neoapp::{App, Notification};
use neoweb_core::library::{
    self, Bookmarks, DownloadState as Done, Downloads, FileKind, History, HistoryRow as HRow,
    Settings,
};
use neoweb_proto::{Command, DownloadOp, DownloadState, Event};
use neoweb_ui::{BookmarkRow, BrowserApp, BrowserState, DownloadRow, HistoryRow};
use slint::ComponentHandle;

/// NEOWEB's private folder (SID 0xE0007001).
const DIR: &str = "C:\\Private\\e0007001\\";
/// The download notifications' ids start here (the app's own numbering).
const NOTIFY_BASE: u32 = 0x1000;

/// Local time now (Unix seconds plus the UTC offset), as the models count it.
pub fn now() -> i64 {
    let (utc, offset) = e32::time::utc_now();
    utc + i64::from(offset)
}

fn load(name: &str) -> String {
    let read = || -> Result<String, i32> {
        let fs = Fs::connect()?;
        let f = File::open(
            &fs,
            &format!("{DIR}{name}"),
            mode::READ | mode::SHARE_READERS_ONLY,
        )?;
        let mut buf = alloc::vec![0u8; f.size()? as usize];
        let n = f.read_at(0, &mut buf)?;
        buf.truncate(n);
        Ok(String::from_utf8_lossy(&buf).into())
    };
    read().unwrap_or_default()
}

fn save(name: &str, text: &str) {
    // a temporary file replaces the old one only once written: a full disk keeps the old library (review #12)
    let write = || -> Result<(), i32> {
        let fs = Fs::connect()?;
        let _ = fs.mkdir(DIR);
        let tmp = format!("{DIR}{name}.tmp");
        {
            let f = File::replace(&fs, &tmp, mode::SHARE_EXCLUSIVE)?;
            f.write(text.as_bytes())?;
            f.flush()?;
        }
        fs.replace_file(&tmp, &format!("{DIR}{name}"))
    };
    if let Err(e) = write() {
        e32::dprintln!("neoweb: saving {} failed {}", name, e);
    }
}

/// What a library action asks of the browser.
pub enum Then {
    Nothing,
    /// Load this address (the library view closes).
    Go(String),
    /// Ask "Clear browsing data?" (a destructive QUERY).
    AskClear,
    /// A prompt sheet: (kind, its question, its text).
    Prompt(Rename, &'static str, String),
    /// Move bookmark `i`: a choice of folders.
    Move(usize),
}

/// What a library prompt names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Rename {
    NewFolder,
    Folder(u32),
    Bookmark(usize),
}

#[derive(Default)]
pub struct Library {
    pub bookmarks: Bookmarks,
    pub history: History,
    pub downloads: Downloads,
    pub settings: Settings,
    /// engine download id -> the model's id
    ids: Vec<(u32, u32)>,
    /// the bookmark folder shown (None = the top level), the row long-pressed
    folder: Option<u32>,
    long: Option<usize>,
    /// last progress per download: (model id, bytes, ms) for the rate
    rates: Vec<(u32, u64, u64, u64)>,
    dirty: [bool; 4],
}

impl Library {
    pub fn load() -> Self {
        Library {
            bookmarks: Bookmarks::decode(&load("bookmarks.txt")),
            history: History::decode(&load("history.txt")),
            downloads: Downloads::decode(&load("downloads.txt")),
            settings: Settings::decode(&load("settings.txt")),
            ..Library::default()
        }
    }

    /// Write what changed.
    pub fn save(&mut self) {
        let [b, h, d, s] = core::mem::take(&mut self.dirty);
        if b {
            save("bookmarks.txt", &self.bookmarks.encode());
        }
        if h {
            save("history.txt", &self.history.encode());
        }
        if d {
            save("downloads.txt", &self.downloads.encode());
        }
        if s {
            save("settings.txt", &self.settings.encode());
        }
    }

    pub fn visited(&mut self, title: &str, url: &str) {
        if !url.is_empty() && !url.starts_with("neo:") {
            self.history.add(title, url, now());
            self.dirty[1] = true;
        }
    }

    pub fn search(&self, text: &str) -> String {
        self.settings.engine.search(text)
    }

    /// The ☆ in the address bar: true when the page is a bookmark now.
    pub fn toggle_bookmark(&mut self, title: &str, url: &str) -> bool {
        self.dirty[0] = true;
        if self.bookmarks.is_bookmarked(url) {
            self.bookmarks.remove_url(url);
            false
        } else {
            self.bookmarks
                .add(if title.is_empty() { url } else { title }, url);
            true
        }
    }

    /// The commands for the engine that carry the settings (at start and after a restart).
    pub fn engine_settings(&self) -> [Command; 2] {
        [
            Command::DesktopSite(self.settings.desktop_site),
            Command::Zoom(u32::from(self.settings.text_size)),
        ]
    }

    /// Put the library into the UI (the shown view's rows, the settings).
    pub fn show(&self, ui: &BrowserApp) {
        let s = ui.global::<BrowserState>();
        let query: String = s.get_library_query().into();
        let q = query.to_lowercase();
        let folders: Vec<BookmarkRow> = if self.folder.is_none() && q.is_empty() {
            self.bookmarks
                .folders
                .iter()
                .map(|f| BookmarkRow {
                    title: f.name.as_str().into(),
                    detail: self.bookmarks.count_label(f.id).as_str().into(),
                    letter: "".into(),
                    color: slint::Color::default(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let marks: Vec<BookmarkRow> = self
            .shown_bookmarks(&q)
            .into_iter()
            .map(|i| {
                let b = &self.bookmarks.list[i];
                row(&b.title, &b.url, bare(&b.url))
            })
            .collect();
        s.set_bookmark_folder(
            self.folder
                .and_then(|f| self.bookmarks.folder(f))
                .map_or("".into(), |f| f.name.as_str().into()),
        );
        s.set_bookmark_folders(crate::model(folders));
        s.set_bookmarks(crate::model(marks));
        let rows: Vec<HistoryRow> = self
            .history
            .rows(now(), &query)
            .into_iter()
            .map(|r| match r {
                HRow::Day(label) => HistoryRow {
                    section: true,
                    title: label.as_str().into(),
                    detail: "".into(),
                    letter: "".into(),
                    color: slint::Color::default(),
                },
                HRow::Visit { index, detail } => {
                    let v = &self.history.list[index];
                    let r = row(&v.title, &v.url, &detail);
                    HistoryRow {
                        section: false,
                        title: r.title,
                        detail: r.detail,
                        letter: r.letter,
                        color: r.color,
                    }
                }
            })
            .collect();
        s.set_history_rows(crate::model(rows));
        let (going, done) = self.downloads.sections();
        let dl = |d: &library::Download| DownloadRow {
            name: d.name.as_str().into(),
            detail: d.detail(self.rate(d.id), now()).as_str().into(),
            state: match d.state {
                Done::Downloading => 0,
                Done::Paused => 1,
                Done::Done => 2,
                Done::Failed => 3,
            },
            kind: match d.kind() {
                FileKind::Doc => 0,
                FileKind::Image => 1,
                FileKind::File => 2,
            },
            progress: d.fraction(),
        };
        s.set_downloading(crate::model(going.into_iter().map(dl).collect()));
        s.set_downloaded(crate::model(done.into_iter().map(dl).collect()));
        let st = &self.settings;
        s.set_settings_engine(st.engine as i32);
        s.set_settings_text_size(i32::from(st.text_size));
        s.set_settings_cookies(st.cookies as i32);
        s.set_settings_desktop(st.desktop_site);
        s.set_settings_js(st.javascript);
        s.set_settings_unsafe(st.allow_unsafe_certs);
    }

    /// The bookmark indices shown: the open folder's, or every match of a search.
    fn shown_bookmarks(&self, q: &str) -> Vec<usize> {
        if q.is_empty() {
            self.bookmarks
                .in_folder(self.folder)
                .map(|(i, _)| i)
                .collect()
        } else {
            (0..self.bookmarks.list.len())
                .filter(|&i| {
                    let b = &self.bookmarks.list[i];
                    b.title.to_lowercase().contains(q) || b.url.contains(q)
                })
                .collect()
        }
    }

    /// Bytes a second of download `id` lately.
    fn rate(&self, id: u32) -> u64 {
        self.rates.iter().find(|r| r.0 == id).map_or(0, |r| r.3)
    }

    /// A library action (UI4's acts). `app` for opening files; `out` gets engine commands.
    pub fn act(
        &mut self,
        app: &App,
        ui: &BrowserApp,
        action: &str,
        i: i32,
        out: &mut Vec<Command>,
    ) -> Then {
        let s = ui.global::<BrowserState>();
        let iu = i.max(0) as usize;
        let q: String = String::from(s.get_library_query().as_str()).to_lowercase();
        match action {
            "bm-open" => {
                if let Some(&b) = self.shown_bookmarks(&q).get(iu) {
                    return Then::Go(self.bookmarks.list[b].url.clone());
                }
            }
            "bm-folder" => self.folder = self.bookmarks.folders.get(iu).map(|f| f.id),
            "bm-new-folder" => {
                return Then::Prompt(Rename::NewFolder, "Folder name:", String::new());
            }
            "bm-long" => self.long = self.shown_bookmarks(&q).get(iu).copied(),
            "bm-edit" => {
                if let Some(b) = self.long.and_then(|b| self.bookmarks.list.get(b)) {
                    return Then::Prompt(
                        Rename::Bookmark(self.long.unwrap()),
                        "Bookmark name:",
                        b.title.clone(),
                    );
                }
            }
            "bm-move" => {
                if let Some(b) = self.long {
                    return Then::Move(b);
                }
            }
            "bm-delete" => {
                if let Some(b) = self.long.take() {
                    self.bookmarks.remove(b);
                    self.dirty[0] = true;
                }
            }
            "bm-folder-long" => self.long = Some(iu),
            "bm-folder-rename" => {
                if let Some(f) = self.bookmarks.folders.get(iu) {
                    return Then::Prompt(Rename::Folder(f.id), "Folder name:", f.name.clone());
                }
            }
            "bm-folder-delete" => {
                if let Some(id) = self.bookmarks.folders.get(iu).map(|f| f.id) {
                    self.bookmarks.remove_folder(id);
                    self.dirty[0] = true;
                }
            }
            "library-back" => self.folder = None,
            "library-menu" => match (s.get_library(), i) {
                // Downloads' ⋮: 0 Clear the list, 1 Settings (review #13)
                (3, 0) => {
                    let done: Vec<u32> = self.downloads.sections().1.iter().map(|d| d.id).collect();
                    for id in done {
                        self.downloads.remove(id);
                    }
                    self.dirty[2] = true;
                }
                _ => ui.invoke_show_library(4),
            },
            "hist-open" | "hist-remove" => {
                if let Some(HRow::Visit { index, .. }) =
                    self.history.rows(now(), &q).into_iter().nth(iu)
                {
                    if action == "hist-open" {
                        return Then::Go(self.history.list[index].url.clone());
                    }
                    self.history.remove(index);
                    self.dirty[1] = true;
                }
            }
            "hist-clear" | "set-clear" => return Then::AskClear,
            "dl-pause" | "dl-cancel" => {
                let Some(id) = self.downloads.sections().0.get(iu).map(|d| d.id) else {
                    return Then::Nothing;
                };
                let engine = self.ids.iter().find(|e| e.1 == id).map(|e| e.0);
                let Some(d) = self.downloads.get(id).cloned() else {
                    return Then::Nothing;
                };
                match (action, engine, d.state) {
                    ("dl-cancel", Some(e), _) => {
                        out.push(Command::DownloadOp {
                            id: e,
                            op: DownloadOp::Cancel,
                        });
                        self.downloads.remove(id);
                    }
                    ("dl-cancel", None, _) => self.downloads.remove(id),
                    // a failed one, or one from before the engine started again: from the start
                    (_, None, _) | (_, _, Done::Failed) => {
                        self.downloads.remove(id);
                        out.push(Command::Download { url: d.url });
                    }
                    (_, Some(e), Done::Paused) => out.push(Command::DownloadOp {
                        id: e,
                        op: DownloadOp::Resume,
                    }),
                    (_, Some(e), _) => out.push(Command::DownloadOp {
                        id: e,
                        op: DownloadOp::Pause,
                    }),
                }
                self.dirty[2] = true;
            }
            "dl-open" => {
                if let Some(d) = self.downloads.sections().1.get(iu) {
                    open_in_files(app, &d.path);
                }
            }
            "set-engine" => {
                if let Some(e) = library::SearchEngine::from_index(i) {
                    self.settings.engine = e;
                }
            }
            "set-text-size" => {
                if let Some(&p) = library::TEXT_SIZES.get(iu) {
                    self.settings.set_text_size(i64::from(p));
                    out.push(Command::Zoom(u32::from(p)));
                }
            }
            "set-desktop" => {
                self.settings.desktop_site = i != 0;
                out.push(Command::DesktopSite(i != 0));
            }
            // shortcut: Servo 0.8 has no switch for script; the setting is kept for when it does
            "set-js" => self.settings.javascript = i != 0,
            // shortcut: kept for WB6's cookie policy (Servo 0.8 takes no third-party rule)
            "set-cookies" => {
                self.settings.cookies = if i == 0 {
                    library::Cookies::AllowAll
                } else {
                    library::Cookies::BlockThirdParty
                }
            }
            // B10: read by WB6's certificate errors
            "set-unsafe" => self.settings.allow_unsafe_certs = i != 0,
            _ => return Then::Nothing,
        }
        if action.starts_with("set-") {
            self.dirty[3] = true;
        }
        Then::Nothing
    }

    /// A prompt's answer for the library.
    pub fn renamed(&mut self, what: Rename, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        match what {
            Rename::NewFolder => {
                self.bookmarks.add_folder(text);
            }
            Rename::Folder(id) => self.bookmarks.rename_folder(id, text),
            Rename::Bookmark(i) => {
                if let Some(url) = self.bookmarks.list.get(i).map(|b| b.url.clone()) {
                    self.bookmarks.edit(i, text, &url);
                }
            }
        }
        self.dirty[0] = true;
    }

    /// Bookmark `i` into the folder chosen (0 = none, else the n-th folder).
    pub fn moved(&mut self, i: usize, choice: usize) {
        let folder = choice
            .checked_sub(1)
            .and_then(|f| self.bookmarks.folders.get(f))
            .map(|f| f.id);
        self.bookmarks.move_to(i, folder);
        self.dirty[0] = true;
    }

    /// The folder choices for Move (first "No folder").
    pub fn folder_names(&self) -> Vec<String> {
        core::iter::once(String::from("No folder"))
            .chain(self.bookmarks.folders.iter().map(|f| f.name.clone()))
            .collect()
    }

    pub fn cleared(&mut self) {
        self.history.clear();
        self.dirty[1] = true;
    }

    /// The engine's report on a download: the model, the rate and the shade entry (with its progress bar).
    pub fn download(&mut self, app: &App, e: Event) {
        let Event::Download {
            id,
            name,
            url,
            path,
            received,
            total,
            state,
        } = e
        else {
            return;
        };
        let mid = match self.ids.iter().find(|x| x.0 == id) {
            Some(x) => x.1,
            None => {
                let m = self.downloads.add(&name, &url, &path, total);
                self.ids.push((id, m));
                m
            }
        };
        let ms = e32::time::millis();
        match self.rates.iter_mut().find(|r| r.0 == mid) {
            Some(r) if ms > r.2 + 500 => {
                r.3 = (received.saturating_sub(r.1)) * 1000 / (ms - r.2);
                (r.1, r.2) = (received, ms);
            }
            Some(_) => {}
            None => self.rates.push((mid, received, ms, 0)),
        }
        self.downloads.progress(mid, received);
        let mut n = Notification {
            id: NOTIFY_BASE + mid,
            title: "Downloading".into(),
            text: format!("Browser · {name}"),
            page: 3, // the Downloads view (Open { page })
            ..Notification::default()
        };
        match state {
            DownloadState::Downloading => {
                n.progress = (total > 0).then(|| (received * 100 / total.max(1)) as u8);
            }
            DownloadState::Paused => {
                self.downloads.toggle_pause(mid);
                n.title = "Download paused".into();
            }
            DownloadState::Done => {
                self.downloads.finish(mid, now());
                n.title = "Download complete".into();
                n.text = format!("Browser · {name} · {}", library::size_text(received));
                n.actions = alloc::vec!["Open".into()];
                self.dirty[2] = true;
            }
            DownloadState::Failed => {
                self.downloads.fail(mid);
                n.title = "Download failed".into();
                self.dirty[2] = true;
            }
            DownloadState::Cancelled => {
                self.downloads.remove(mid);
                let _ = app.cancel_notify(NOTIFY_BASE + mid);
                self.dirty[2] = true;
                return;
            }
        }
        let _ = app.notify(&n);
    }

    /// A tap on a download's shade entry (or its Open): the file in Files, else the Downloads view.
    pub fn notified(&mut self, app: &App, ui: &BrowserApp, id: u32, action: i32) {
        let d = id
            .checked_sub(NOTIFY_BASE)
            .and_then(|m| self.downloads.get(m));
        // action 0 is the entry itself, 1 its first button (Open)
        match d {
            Some(d) if action == 1 && d.state == Done::Done => open_in_files(app, &d.path),
            _ => {
                ui.invoke_show_library(3);
                self.show(ui);
            }
        }
    }
}

/// Files shows a downloaded file (protocol 1.12 OPEN_FILE).
fn open_in_files(app: &App, path: &str) {
    let open = || -> Result<(), i32> {
        let mut fs = Fs::connect()?;
        fs.share_protected()?;
        let f = File::open(
            &fs,
            &path.replace('/', "\\"),
            mode::READ | mode::SHARE_READERS_ONLY,
        )?;
        app.open_file(neoui_proto::open::files::KEY, 0, &f)
    };
    if let Err(e) = open() {
        e32::dprintln!("neoweb: open {} in Files failed {}", path, e);
        let _ = app.note("Can't open the file", "");
    }
}

fn bare(url: &str) -> &str {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
}

/// A library row: title (or address), detail, a letter badge coloured by the site.
fn row(title: &str, url: &str, detail: &str) -> BookmarkRow {
    let domain = library::domain(url);
    BookmarkRow {
        title: if title.is_empty() { bare(url) } else { title }.into(),
        detail: detail.into(),
        letter: String::from(
            domain
                .chars()
                .next()
                .map_or('·', |c| c.to_ascii_uppercase()),
        )
        .as_str()
        .into(),
        color: slint::Color::from_argb_encoded(crate::badge(&domain)),
    }
}
