// SPDX-License-Identifier: GPL-3.0-only
// UI3 (docs browser/ui-screens.md, WB5a): forms with the keyboard up, the script prompt, a <select>, the upload choice and
// Save password; boards br2-form, br2-js-prompt, br2-upload, br2-login-save (br2-js-alert is the shell's QUERY).
// The select sheet has no board; its snapshot is br2x-select.
mod common;
use common::{DP, GESTURES};
use neoweb_ui::{RecentFile, SelectRow};
use slint::{ModelRc, VecModel};
use std::rc::Rc;

/// The keyboard of the boards: Armenian, 276 dp.
const KEYBOARD_DP: u32 = 276;

fn page(s: &common::Shot, fake: i32, domain: &str) {
    let st = s.state();
    st.set_page(fake);
    st.set_security(1);
    st.set_domain(domain.into());
    st.set_count("2".into());
    st.set_can_back(true);
}

#[test]
fn ui3_boards() {
    let s = common::app();
    let st = s.state();

    // br2-form: the keyboard covers the footer; the page ends at it (679 − 48 − 276 = 355 dp)
    page(&s, 2, "github.com");
    st.set_keyboard_px((KEYBOARD_DP as f32 * DP) as i32);
    s.settle();
    assert_eq!(s.ui.get_page_height_px(), (355.0 * DP) as i32);
    s.shoot_at("br2-form", GESTURES);

    // br2-js-prompt: the sheet sits on the keyboard; OK answers with the field's text
    page(&s, 3, "forum.example.org");
    st.set_sheet_site("forum.example.org".into());
    st.set_sheet_text("Name for the new folder:".into());
    st.set_sheet_value("Browser".into());
    s.ui.invoke_show_sheet(1);
    s.settle();
    s.shoot_at("br2-js-prompt", GESTURES);
    s.take_acts();
    let sheet_bottom = (679 - KEYBOARD_DP) as f32;
    s.tap(308.0, sheet_bottom - 12.0 - 4.0 - 20.0); // OK
    assert_eq!(s.take_acts(), [("prompt".to_string(), 1)]);
    assert!(!s.ui.get_sheet_open());
    assert_eq!(st.get_sheet_value(), "Browser");

    // the prompt again: a tap outside cancels (a draw runs the sheet's change handlers)
    s.ui.invoke_show_sheet(1);
    s.draw_at(GESTURES);
    s.tap(180.0, 80.0);
    assert_eq!(s.take_acts(), [("prompt".to_string(), 0)]);
    st.set_keyboard_px(0);

    // br2-upload: Photos · Files · Memory card, then recent files
    let recent = vec![
        RecentFile {
            name: "trip-notes.txt".into(),
            detail: "4 KB · 3 Nov".into(),
            image: false,
        },
        RecentFile {
            name: "boarding-pass.pdf".into(),
            detail: "212 KB · 1 Nov".into(),
            image: false,
        },
        RecentFile {
            name: "sunset.jpg".into(),
            detail: "3.1 MB · 29 Oct".into(),
            image: true,
        },
    ];
    st.set_recent_files(ModelRc::from(Rc::new(VecModel::from(recent))));
    s.ui.invoke_show_sheet(3);
    s.settle();
    s.shoot_at("br2-upload", GESTURES);
    s.take_acts();
    s.tap(180.0, 679.0 - 12.0 - 3.0 * 56.0 - 28.0 - 6.0 - 36.0); // Files
    s.ui.invoke_show_sheet(3);
    s.draw_at(GESTURES);
    s.tap(180.0, 679.0 - 12.0 - 28.0); // the last recent file
    assert_eq!(
        s.take_acts(),
        [("upload".to_string(), 1), ("upload".to_string(), 12)]
    );

    // br2-login-save: Save is off (B11); Not now answers 1
    page(&s, 2, "github.com");
    st.set_sheet_site("github.com".into());
    st.set_sheet_text("ava@example.org".into());
    s.ui.invoke_show_sheet(4);
    s.settle();
    s.shoot_at("br2-login-save", GESTURES);
    s.take_acts();
    s.tap(308.0, 679.0 - 12.0 - 4.0 - 20.0); // Save: disabled
    assert!(s.take_acts().is_empty());
    s.tap(180.0, 80.0); // outside = Not now
    assert_eq!(s.take_acts(), [("login".to_string(), 1)]);

    // a <select> with a group and a disabled option; a tap picks and closes
    let row = |label: &str, group: bool, disabled: bool, selected: bool| SelectRow {
        label: label.into(),
        group,
        disabled,
        selected,
    };
    let rows = vec![
        row("Any size", false, false, true),
        row("Phones", true, false, false),
        row("Pixel 3a XL", false, false, false),
        row("Nokia N8", false, true, false),
    ];
    st.set_select_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
    st.set_select_multiple(false);
    s.ui.invoke_show_sheet(2);
    s.settle();
    s.shoot_at("br2x-select", GESTURES);
    s.take_acts();
    s.tap(180.0, 679.0 - 12.0 - 56.0 - 28.0); // Pixel 3a XL (above the disabled N8)
    assert_eq!(s.take_acts(), [("select".to_string(), 2)]);
    assert!(!s.ui.get_sheet_open());
}
