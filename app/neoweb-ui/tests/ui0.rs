// SPDX-License-Identifier: GPL-3.0-only
// UI0 (docs browser-ui.md): the start page and a page with the chrome, in Gestures and Buttons mode, and every header
// and footer state; boards br2-start, br2-start-buttons, br2-page, br2-page-buttons, br2-footer-states.
mod common;
use common::{BUTTONS, GESTURES, Shot, at, save, strings};
use neoweb_core::{FakePage, Header, Security, count_label};

fn show_page(s: &Shot, url: &str, count: usize, back: bool, forward: bool) {
    let st = s.state();
    let h = Header::of(url);
    let page = match h.domain.as_str() {
        "" => FakePage::Start,
        "en.wikipedia.org" => FakePage::Wikipedia,
        "github.com" => FakePage::GitHub,
        _ => FakePage::Other,
    };
    st.set_page(page as i32);
    st.set_security(h.security as i32);
    st.set_domain(h.domain.into());
    st.set_count(count_label(count).into());
    st.set_can_back(back);
    st.set_can_forward(forward);
    st.set_loading(false);
    st.set_bookmarked(false);
}

/// The 48 dp band at `y_dp` of a frame `h_dp` high.
fn band(px: &[u32], y_dp: u32) -> &[u32] {
    let w = common::W as usize;
    let (a, b) = ((y_dp * 3) as usize * w, ((y_dp + 48) * 3) as usize * w);
    &px[a..b]
}

#[test]
fn ui0_boards() {
    let s = common::app();
    let st = s.state();

    // br2-start: the empty header, the 4 × 2 shortcuts, Recently visited; ← → dim, one page
    show_page(&s, neoweb_core::START, 1, false, false);
    st.set_recents(strings(&[
        "github.com/NeoSymbian",
        "symbian.example.org/wiki/EKA2",
        "slint.dev/docs",
    ]));
    let px = s.shoot_at("br2-start", GESTURES);
    assert_eq!(at(&px, 340.0, 6.0), 0x0b0c0e, "the header's colour");
    s.shoot_at("br2-start-buttons", BUTTONS);

    // a tap on the first shortcut (NeoSymbian, at 12 + 1/8 of the width, the grid's first row) and on the header
    s.draw_at(GESTURES);
    s.take_acts();
    s.tap(48.0, 95.0);
    s.tap(180.0, 24.0);
    assert_eq!(
        s.take_acts(),
        [("shortcut".to_string(), 0), ("address".to_string(), 0)]
    );

    // br2-page: Wikipedia, secure, three pages open, ← on
    show_page(&s, "https://en.wikipedia.org/wiki/Symbian", 3, true, false);
    let px = s.shoot_at("br2-page", GESTURES);
    assert_eq!(
        at(&px, 180.0, 300.0),
        0xffffff,
        "the page is white under the chrome"
    );
    s.shoot_at("br2-page-buttons", BUTTONS);
    s.draw_at(GESTURES);
    s.take_acts();
    // the footer: ← (slot 1 of 5) works, → (slot 2) is disabled
    s.tap(36.0, 679.0 - 24.0);
    s.tap(108.0, 679.0 - 24.0);
    assert_eq!(s.take_acts(), [("back".to_string(), 0)]);

    // br2-footer-states: headers (empty, secure, bookmarked, not secure, loading), then footers (start, mid-history,
    // many pages), stacked
    let mut sheet: Vec<u32> = Vec::new();
    let header = |sheet: &mut Vec<u32>, url: &str, bookmarked: bool, loading: bool| {
        show_page(&s, url, 3, true, false);
        st.set_bookmarked(bookmarked);
        st.set_loading(loading);
        st.set_progress(0.4);
        sheet.extend_from_slice(band(&s.draw_at(GESTURES), 0));
    };
    header(&mut sheet, neoweb_core::START, false, false);
    header(
        &mut sheet,
        "https://en.wikipedia.org/wiki/Symbian",
        false,
        false,
    );
    header(
        &mut sheet,
        "https://en.wikipedia.org/wiki/Symbian",
        true,
        false,
    );
    header(&mut sheet, "http://symbian.example.org/", false, false);
    header(
        &mut sheet,
        "https://en.wikipedia.org/wiki/Symbian",
        false,
        true,
    );
    for (count, back, forward) in [(1, false, false), (3, true, true), (12, true, false)] {
        show_page(
            &s,
            "https://en.wikipedia.org/wiki/Symbian",
            count,
            back,
            forward,
        );
        sheet.extend_from_slice(band(&s.draw_at(GESTURES), GESTURES - 48));
    }
    save("br2-footer-states", &sheet, 8 * 48 * 3);
    assert_eq!(
        Header::of("http://symbian.example.org/").security,
        Security::NotSecure
    );
}
