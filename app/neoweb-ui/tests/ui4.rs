// SPDX-License-Identifier: GPL-3.0-only
// UI4 (docs browser/ui-screens.md, WB5d): the library and settings, full-screen views over the browser; boards
// br2-bookmarks, br2-history, br2-downloads, br2-settings. Each is a kit layer: Back closes it (or leaves a folder).
mod common;
use common::GESTURES;
use neoweb_ui::{BookmarkRow, DownloadRow, HistoryRow, Kit};
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

fn rgb(c: u32) -> Color {
    Color::from_argb_encoded(0xff00_0000 | c)
}

fn acts(v: &[(&str, i32)]) -> Vec<(String, i32)> {
    v.iter().map(|(a, i)| (a.to_string(), *i)).collect()
}

/// Back from the system: bump Kit.back-seq; the top layer answers on the next draw.
fn back(s: &common::Shot) {
    let kit = s.ui.global::<Kit>();
    kit.set_back_seq(kit.get_back_seq() + 1);
    s.draw_at(GESTURES);
}

#[test]
fn ui4_boards() {
    let s = common::app();
    let st = s.state();
    let kit = s.ui.global::<Kit>();

    // br2-bookmarks: two folders, then five bookmarks
    let row = |title: &str, detail: &str, letter: &str, color: u32| BookmarkRow {
        title: title.into(),
        detail: detail.into(),
        letter: letter.into(),
        color: rgb(color),
    };
    st.set_bookmark_folders(model(vec![
        row("Symbian docs", "6 bookmarks", "", 0),
        row("Reading list", "4 bookmarks", "", 0),
    ]));
    st.set_bookmarks(model(vec![
        row(
            "Symbian — Wikipedia",
            "en.wikipedia.org/wiki/Symbian",
            "W",
            0x5b6472,
        ),
        row("NeoSymbian", "github.com/NeoSymbian", "G", 0x3a3f48),
        row("Slint documentation", "slint.dev/docs", "S", 0xe2a92a),
        row(
            "EKA2 — Symbian wiki",
            "symbian.example.org/wiki/EKA2",
            "E",
            0x8656e8,
        ),
        row(
            "Servo, the embeddable web engine",
            "servo.org",
            "Sv",
            0x22b46a,
        ),
    ]));
    s.ui.invoke_show_library(1);
    assert_eq!(st.get_library(), 1);
    assert_eq!(kit.get_layers(), 1);
    s.shoot_at("br2-bookmarks", GESTURES);
    s.take_acts();
    s.tap(180.0, 82.0 + 28.0); // Symbian docs
    s.tap(180.0, 228.0 + 72.0 + 36.0); // NeoSymbian
    s.tap(284.0, 24.0); // + new folder
    assert_eq!(
        s.take_acts(),
        acts(&[("bm-folder", 0), ("bm-open", 1), ("bm-new-folder", 0)])
    );
    // a hold: Edit · Move · Delete at the finger
    s.long_press(120.0, 228.0 + 2.0 * 72.0 + 36.0);
    assert_eq!(s.take_acts(), acts(&[("bm-long", 2)]));
    s.draw_at(GESTURES);
    s.tap(
        120.0 + 30.0,
        228.0 + 2.0 * 72.0 + 36.0 + 8.0 + 2.0 * 48.0 + 24.0,
    ); // Delete
    assert_eq!(s.take_acts(), acts(&[("bm-delete", 2)]));
    // the search field: each change is "bm-search" with the query in library-query
    s.tap(236.0, 24.0);
    s.draw_at(GESTURES);
    assert_eq!(kit.get_layers(), 2);
    back(&s); // closes the search first
    assert_eq!(kit.get_layers(), 1);
    assert_eq!(st.get_library(), 1);
    s.take_acts();

    // in a folder: its name is the title; Back goes up (the app shows the top level), not out
    st.set_bookmark_folder("Symbian docs".into());
    st.set_bookmark_folders(model(Vec::new()));
    back(&s);
    assert_eq!(s.take_acts(), acts(&[("library-back", 0)]));
    assert_eq!(st.get_library(), 1);
    st.set_bookmark_folder("".into());
    s.tap(24.0, 24.0); // ← at the top level closes
    assert_eq!(s.take_acts(), acts(&[("library-back", 0)]));
    assert_eq!(st.get_library(), 0);
    assert_eq!(kit.get_layers(), 0);

    // br2-history: Clear browsing data, then Today and Yesterday
    let visit = |title: &str, detail: &str, letter: &str, color: u32| HistoryRow {
        section: false,
        title: title.into(),
        detail: detail.into(),
        letter: letter.into(),
        color: rgb(color),
    };
    let day = |label: &str| HistoryRow {
        section: true,
        title: label.into(),
        detail: "".into(),
        letter: "".into(),
        color: rgb(0),
    };
    st.set_history_rows(model(vec![
        day("Today"),
        visit(
            "Symbian — Wikipedia",
            "en.wikipedia.org · 10:21",
            "W",
            0x5b6472,
        ),
        visit("NeoSymbian", "github.com · 10:05", "G", 0x3a3f48),
        visit("Slint documentation", "slint.dev · 09:48", "S", 0xe2a92a),
        day("Yesterday"),
        visit(
            "EKA2 — Symbian wiki",
            "symbian.example.org · 22:13",
            "E",
            0x8656e8,
        ),
        visit(
            "Servo, the embeddable web engine",
            "servo.org · 18:40",
            "Sv",
            0x22b46a,
        ),
        visit("NeoSymbian/browser", "github.com · 18:32", "G", 0x3a3f48),
    ]));
    s.ui.invoke_show_library(2);
    s.shoot_at("br2-history", GESTURES);
    s.take_acts();
    s.tap(180.0, 76.0); // Clear browsing data
    s.tap(180.0, 138.0 + 64.0 + 32.0); // NeoSymbian
    s.tap(328.0, 364.0 + 32.0); // ✕ on EKA2
    assert_eq!(
        s.take_acts(),
        acts(&[("hist-clear", 0), ("hist-open", 2), ("hist-remove", 5)])
    );

    // br2-downloads: one going, three done
    let dl = |name: &str, detail: &str, state: i32, kind: i32, progress: f32| DownloadRow {
        name: name.into(),
        detail: detail.into(),
        state,
        kind,
        progress,
    };
    st.set_downloading(model(vec![dl(
        "symbian3-dev-guide.pdf",
        "3.2 of 8.4 MB · 13 s left",
        0,
        0,
        0.38,
    )]));
    st.set_downloaded(model(vec![
        dl("EKA2-internals.pdf", "2.4 MB · Today 09:52", 2, 0, 1.0),
        dl("n8-wallpaper.jpg", "860 KB · Today 09:30", 2, 1, 1.0),
        dl("frozen-bubble.sis", "1.9 MB · Yesterday", 2, 2, 1.0),
    ]));
    s.ui.invoke_show_library(3);
    s.shoot_at("br2-downloads", GESTURES);
    s.take_acts();
    s.tap(274.0, 126.0); // pause
    s.tap(328.0, 126.0); // ✕
    s.tap(318.0, 204.0 + 72.0 + 36.0); // Open on the wallpaper
    s.tap(150.0, 348.0 + 36.0); // the .sis row
    assert_eq!(
        s.take_acts(),
        acts(&[
            ("dl-pause", 0),
            ("dl-cancel", 0),
            ("dl-open", 1),
            ("dl-open", 2)
        ])
    );

    // br2-settings: General, Content, Privacy (with B10's switch, off), About
    s.ui.invoke_show_library(4);
    assert_eq!(kit.get_layers(), 1);
    s.shoot_at("br2-settings", GESTURES);
    s.take_acts();
    s.tap(100.0, 262.0); // Desktop site by default (the row)
    s.tap(321.0, 368.0); // JavaScript's switch
    s.tap(100.0, 538.0); // Clear browsing data
    s.tap(321.0, 602.0); // allow unsafe certificates
    assert_eq!(
        s.take_acts(),
        acts(&[
            ("set-desktop", 1),
            ("set-js", 0),
            ("set-clear", 0),
            ("set-unsafe", 1)
        ])
    );
    assert!(st.get_settings_desktop() && !st.get_settings_js() && st.get_settings_unsafe());
    // Search engine: a ChoiceSheet; Google is the second of four rows
    s.tap(100.0, 118.0);
    s.draw_at(GESTURES);
    s.tap(180.0, 679.0 - 12.0 - 2.0 * 56.0 - 28.0);
    assert_eq!(s.take_acts(), acts(&[("set-engine", 1)]));
    // Cookies: Allow all is the first of two
    s.draw_at(GESTURES);
    s.tap(100.0, 474.0);
    s.draw_at(GESTURES);
    s.tap(180.0, 679.0 - 12.0 - 2.0 * 56.0 + 28.0);
    assert_eq!(s.take_acts(), acts(&[("set-cookies", 0)]));
    // Text size: 115 % is the fourth of six
    s.draw_at(GESTURES);
    s.tap(100.0, 190.0);
    s.draw_at(GESTURES);
    s.tap(180.0, 679.0 - 12.0 - 3.0 * 56.0 + 28.0);
    assert_eq!(s.take_acts(), acts(&[("set-text-size", 3)]));
    s.draw_at(GESTURES);
    back(&s);
    assert_eq!(s.take_acts(), acts(&[("library-back", 0)]));
    assert_eq!(st.get_library(), 0);
    assert_eq!(kit.get_layers(), 0);
}
