// SPDX-License-Identifier: GPL-3.0-only
// UI2 (docs browser/ui-screens.md, WB5c): Opened pages, the Options panel, the link's long-press menu, the text selection bar
// and find in page; boards br2-tabs, br2-menu, br2-link-menu, br2-select, br2-find.
mod common;
use common::{DP, GESTURES, at};
use neoweb_ui::{Kit, Spot, TabCard};
use slint::platform::WindowEvent;
use slint::{Color, ComponentHandle, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, VecModel};
use std::rc::Rc;

/// The keyboard of the boards: 276 dp.
const KEYBOARD_DP: u32 = 276;

fn wiki(s: &common::Shot) {
    let st = s.state();
    st.set_page(1);
    st.set_security(1);
    st.set_domain("en.wikipedia.org".into());
    st.set_count("3".into());
    st.set_can_back(true);
}

/// A dark docs page as a thumbnail: lines of text on #14161a.
fn dark_thumb() -> Image {
    let (w, h) = (120u32, 240u32);
    let mut buf = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
    for (n, p) in buf.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = (n as u32 % w, n as u32 / w);
        let line = y >= 24 && y % 12 < 4 && x >= 6 && x < w - 6 - (y * 7 % 40);
        *p = if line {
            Rgb8Pixel::new(0x3a, 0x3f, 0x48)
        } else {
            Rgb8Pixel::new(0x14, 0x16, 0x1a)
        };
    }
    Image::from_rgb8(buf)
}

/// Back from the system: bump Kit.back-seq; the top layer closes on the next draw.
fn back(s: &common::Shot) {
    let kit = s.ui.global::<Kit>();
    kit.set_back_seq(kit.get_back_seq() + 1);
    s.draw_at(GESTURES);
}

#[test]
fn ui2_boards() {
    let s = common::app();
    let st = s.state();
    let kit = s.ui.global::<Kit>();
    wiki(&s);

    // br2-tabs: three pages, the first current; the footer is hidden under the view
    let card = |title: &str, letter: &str, color: u32, thumb: Image, current: bool| TabCard {
        title: title.into(),
        letter: letter.into(),
        color: Color::from_argb_encoded(0xff00_0000 | color),
        thumb,
        current,
    };
    let cards = vec![
        card("Symbian – Wikipedia", "W", 0x5b6472, Image::default(), true),
        card(
            "NeoSymbian · GitHub",
            "G",
            0x24292f,
            Image::default(),
            false,
        ),
        card("Slint docs", "S", 0x2f80ed, dark_thumb(), false),
    ];
    st.set_tabs(ModelRc::from(Rc::new(VecModel::from(cards))));
    st.set_tabs_open(true);
    s.settle();
    let px = s.shoot_at("br2-tabs", GESTURES);
    assert_eq!(kit.get_layers(), 1, "Opened pages is a kit layer");
    assert_eq!(at(&px, 180.0, 679.0 - 24.0), 0, "no footer under the view");
    assert_eq!(
        at(&px, 93.0, 200.0),
        0xffffff,
        "an empty thumbnail is white"
    );
    assert_eq!(at(&px, 11.0, 150.0), 0x3d8bfd, "the current card's ring");
    s.take_acts();
    s.tap(328.0, 82.0); // ✕ on the second card
    s.tap(332.0, 24.0); // ⋮
    s.tap(93.0, 420.0); // the third card
    assert_eq!(
        s.take_acts(),
        [
            ("tab-close".to_string(), 1),
            ("tabs-options".to_string(), 0),
            ("tab".to_string(), 2)
        ]
    );
    assert!(!st.get_tabs_open(), "opening a page closes the view");
    s.draw_at(GESTURES);
    assert_eq!(kit.get_layers(), 0);
    st.set_tabs_open(true);
    s.draw_at(GESTURES);
    s.tap(284.0, 24.0); // +
    assert_eq!(s.take_acts(), [("new-tab".to_string(), 0)]);
    st.set_tabs_open(true);
    s.draw_at(GESTURES);
    back(&s);
    assert_eq!(s.take_acts(), [("tabs-back".to_string(), 0)]);
    assert!(!st.get_tabs_open());
    assert_eq!(kit.get_layers(), 0);

    // br2-menu: the Options panel, 350 dp, Page (5 tiles) and Pages (4)
    s.ui.invoke_show_sheet(6);
    s.settle();
    let px = s.shoot_at("br2-menu", GESTURES);
    assert_eq!(at(&px, 54.0, 463.0 + 4.0), 0x2a2e35, "Desktop site off");
    s.take_acts();
    let tile = |i: usize, first_row_y: f32| {
        (
            54.0 + 84.0 * (i % 4) as f32,
            first_row_y + 84.0 * (i / 4) as f32 + 24.0,
        )
    };
    let (x, y) = tile(1, 379.0);
    s.tap(x, y); // Find in page
    assert!(!s.ui.get_sheet_open(), "a tile closes the panel");
    st.set_desktop_site(true);
    s.ui.invoke_show_sheet(6);
    s.settle();
    let px = s.draw_at(GESTURES);
    assert_ne!(
        at(&px, 54.0, 463.0 + 4.0),
        0x2a2e35,
        "Desktop site on: the accent tint"
    );
    let (x, y) = tile(3, 575.0);
    s.tap(x, y); // Settings
    assert_eq!(
        s.take_acts(),
        [("option".to_string(), 1), ("option".to_string(), 8)]
    );
    st.set_desktop_site(false);

    // br2-link-menu: over the page at the link (y 318 dp, as the board)
    st.set_link_title("Eclipse Public License".into());
    st.set_link_url("en.wikipedia.org/wiki/Eclipse_Public_License".into());
    st.set_link_y(318.0 * DP);
    s.ui.invoke_show_sheet(5);
    s.settle();
    s.shoot_at("br2-link-menu", GESTURES);
    s.take_acts();
    s.tap(180.0, 100.0); // the scrim
    assert_eq!(s.take_acts(), [("link-cancel".to_string(), 0)]);
    assert!(!s.ui.get_sheet_open());
    // a link near the bottom: the menu stays on screen (its bottom 8 dp above the app's)
    st.set_link_y(660.0 * DP);
    s.ui.invoke_show_sheet(5);
    s.draw_at(GESTURES);
    s.tap(180.0, 679.0 - 8.0 - 6.0 - 48.0 - 24.0); // Download link
    assert_eq!(s.take_acts(), [("link".to_string(), 2)]);
    s.ui.invoke_show_sheet(5);
    s.draw_at(GESTURES);
    assert_eq!(kit.get_layers(), 1);
    back(&s);
    assert_eq!(s.take_acts(), [("link-cancel".to_string(), 0)]);
    assert_eq!(kit.get_layers(), 0);

    // br2-select: handles around "operating … It" of the stand-in page, the bar 6 dp above the first line
    st.set_handle_a(Spot {
        x: 255.0 * DP,
        y: 230.0 * DP,
    });
    st.set_handle_b(Spot {
        x: 128.0 * DP,
        y: 276.0 * DP,
    });
    st.set_selection_x(118.0 * DP);
    st.set_selection_y(186.0 * DP);
    st.set_selection_shown(true);
    s.shoot_at("br2-select", GESTURES);
    s.take_acts();
    s.tap(128.0, 206.0); // Copy
    // far right: the bar stays on screen, its right end 8 dp from the edge
    st.set_selection_x(1000.0 * DP);
    s.draw_at(GESTURES);
    s.tap(360.0 - 8.0 - 12.0, 206.0); // Search
    assert_eq!(
        s.take_acts(),
        [("selection".to_string(), 0), ("selection".to_string(), 2)]
    );
    st.set_selection_shown(false);

    // br2-find: the header is the find bar, the keyboard up
    st.set_keyboard_px((KEYBOARD_DP as f32 * DP) as i32);
    st.set_find_text("Symbian".into());
    st.set_find_count("2/7".into());
    st.set_finding(true);
    s.shoot_at("br2-find", GESTURES);
    s.take_acts();
    // the field has the focus: a key types into it
    let w = s.ui.window();
    w.dispatch_event(WindowEvent::KeyPressed { text: "s".into() });
    w.dispatch_event(WindowEvent::KeyReleased { text: "s".into() });
    assert_eq!(st.get_find_text().len(), "Symbians".len());
    s.tap(258.0, 24.0); // ↑
    s.tap(298.0, 24.0); // ↓
    s.tap(338.0, 24.0); // ✕
    assert_eq!(
        s.take_acts(),
        [
            ("find-edited".to_string(), 0),
            ("find-prev".to_string(), 0),
            ("find-next".to_string(), 0),
            ("find-close".to_string(), 0)
        ]
    );
}
