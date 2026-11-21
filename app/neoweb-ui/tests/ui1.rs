// SPDX-License-Identifier: GPL-3.0-only
// UI1 (docs browser/ui-screens.md): editing the address with suggestions, loading, reading with the chrome hidden; boards
// br2-address, br2-loading, br2-reading.
mod common;
use common::{GESTURES, at};
use neoweb_ui::Suggest;
use slint::{ModelRc, VecModel};
use std::rc::Rc;

fn wiki(s: &common::Shot) {
    let st = s.state();
    st.set_page(1);
    st.set_security(1);
    st.set_domain("en.wikipedia.org".into());
    st.set_count("3".into());
    st.set_can_back(true);
}

#[test]
fn ui1_boards() {
    let s = common::app();
    let st = s.state();
    wiki(&s);

    // br2-address: the full URL in the field, two history rows, a bookmark, the web search
    let rows = vec![
        Suggest {
            kind: 0,
            title: "EKA2 – Wikipedia".into(),
            url: "en.wikipedia.org/wiki/EKA2".into(),
        },
        Suggest {
            kind: 0,
            title: "Symbian Foundation – Wikipedia".into(),
            url: "en.wikipedia.org/wiki/Symbian_Foundation".into(),
        },
        Suggest {
            kind: 1,
            title: "NeoSymbian".into(),
            url: "github.com/NeoSymbian".into(),
        },
        Suggest {
            kind: 2,
            title: "en.wikipedia.org/wiki/Symbian".into(),
            url: "".into(),
        },
    ];
    st.set_suggestions(ModelRc::from(Rc::new(VecModel::from(rows))));
    st.set_edit_text("https://en.wikipedia.org/wiki/Symbian".into());
    st.set_editing(true);
    s.shoot_at("br2-address", GESTURES);
    s.take_acts();
    s.tap(180.0, 48.0 + 72.0 + 36.0); // the second row
    s.tap(336.0, 24.0); // ✕
    assert_eq!(
        s.take_acts(),
        [
            ("suggestion".to_string(), 1),
            ("cancel-edit".to_string(), 0)
        ]
    );
    st.set_editing(false);

    // br2-loading: ✕ in place of ⟳, the progress line
    st.set_loading(true);
    st.set_progress(0.4);
    let px = s.shoot_at("br2-loading", GESTURES);
    assert_eq!(at(&px, 20.0, 47.0), 0x3d8bfd, "the accent progress line");
    s.take_acts();
    s.tap(336.0, 24.0);
    assert!(s.take_acts().contains(&("stop".to_string(), 0)));
    st.set_loading(false);

    // br2-reading: scrolled into the article, header and footer away
    st.set_page_y(-330.0 * common::DP);
    st.set_chrome_shown(false);
    let px = s.shoot_at("br2-reading", GESTURES);
    assert_eq!(
        at(&px, 340.0, 4.0),
        0xffffff,
        "no header: the page is there"
    );
    assert_eq!(at(&px, 340.0, 670.0), 0xffffff, "no footer either");
    st.set_chrome_shown(true);
    let px = s.draw_at(GESTURES);
    assert_eq!(at(&px, 340.0, 4.0), 0x0b0c0e, "shown again");
}
