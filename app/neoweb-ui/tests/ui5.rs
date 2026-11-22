// SPDX-License-Identifier: GPL-3.0-only
// UI5 (docs browser/ui-screens.md, WB6): the certificate error page, the No connection page, Site info and the Wi-Fi
// sign-in banner; boards br2-cert-error, br2-offline, br2-siteinfo, br2-captive.
mod common;
use common::{GESTURES, at};

const ACCENT: u32 = 0x3d8bfd;
const UNSAFE: u32 = 0xff6b6f;

/// The first y (dp) at x where the drawing has `color`, from `from` down.
fn find_y(px: &[u32], x: f32, from: f32, color: u32) -> Option<f32> {
    (0..((GESTURES as f32 - from) * 2.0) as i32)
        .map(|i| from + i as f32 / 2.0)
        .find(|&y| at(px, x, y) == color)
}

fn count(px: &[u32], color: u32) -> usize {
    px.iter().filter(|&&p| p == color).count()
}

#[test]
fn ui5_boards() {
    let s = common::app();
    let st = s.state();

    // br2-cert-error: our page; with Settings' switch off (B10) no override anywhere
    st.set_page(6);
    st.set_security(2);
    st.set_domain("expired.badssl.com".into());
    st.set_count("3".into());
    st.set_can_back(true);
    st.set_error_host("expired.badssl.com".into());
    st.set_error_text("sent a certificate that expired on 12 Apr 2015.".into());
    st.set_cert_to("*.badssl.com".into());
    st.set_cert_by("COMODO RSA Domain Validation Secure Server CA".into());
    st.set_cert_valid("9 Apr 2015 – 12 Apr 2015".into());
    st.set_error_code("CERT_DATE_INVALID".into());
    st.set_settings_unsafe(false);
    let px = s.draw_at(GESTURES);
    let icon = count(&px, UNSAFE); // the lock at the top is the same red
    let back = find_y(&px, 180.0, 100.0, ACCENT).expect("Back to safety");
    s.take_acts();
    s.tap(180.0, back + 20.0);
    assert_eq!(s.take_acts(), [("safety".to_string(), 0)]);
    // Details open: still no override while the switch is off
    s.tap(60.0, back + 48.0 + 12.0 + 22.0);
    let px = s.draw_at(GESTURES);
    assert_eq!(count(&px, UNSAFE), icon, "no override with B10 off");
    // with the switch on, the board: the override at the end of Details
    st.set_settings_unsafe(true);
    let px = s.shoot_at("br2-cert-error", GESTURES);
    assert!(count(&px, UNSAFE) > icon, "the override with B10 on");
    let open = (0..120)
        .map(|i| back + 60.0 + i as f32 * 2.0)
        .find(|&y| (24..200).any(|x| at(&px, x as f32, y) == UNSAFE))
        .expect("Open the site anyway");
    s.take_acts();
    s.tap(60.0, open + 4.0);
    assert_eq!(s.take_acts(), [("cert-allow".to_string(), 0)]);

    // br2-offline: Retry and Check Wi-Fi
    st.set_page(7);
    st.set_security(3); // an error page: the info glyph and the domain
    st.set_domain("github.com".into());
    st.set_error_title("No internet connection".into());
    st.set_error_host("github.com".into());
    st.set_error_text("the address could not be looked up".into());
    st.set_error_code("DNS lookup failed · NAME_NOT_RESOLVED".into());
    let px = s.shoot_at("br2-offline", GESTURES);
    let retry = find_y(&px, 90.0, 100.0, ACCENT).expect("Retry");
    s.take_acts();
    s.tap(90.0, retry + 20.0);
    s.tap(270.0, retry + 20.0);
    assert_eq!(
        s.take_acts(),
        [("retry".to_string(), 0), ("check-wifi".to_string(), 0)]
    );

    // br2-siteinfo: the lock's sheet over a page
    st.set_page(1);
    st.set_security(1);
    st.set_domain("en.wikipedia.org".into());
    st.set_site_title("Symbian - Wikipedia".into());
    st.set_site_initial("W".into());
    st.set_site_checked(true);
    st.set_site_tls("TLS 1.3".into());
    st.set_site_by("E6".into());
    st.set_site_until("11 Jan 2027".into());
    st.set_site_cookies("4 cookies".into());
    s.ui.invoke_show_sheet(7);
    let px = s.shoot_at("br2-siteinfo", GESTURES);
    assert!(s.ui.get_sheet_open());
    let clear = find_y(&px, 320.0, 400.0, 0x2a2e35).expect("Clear");
    s.take_acts();
    s.tap(320.0, clear + 10.0);
    assert_eq!(s.take_acts(), [("site-clear".to_string(), 0)]);
    s.ui.invoke_show_sheet(0);

    // br2-captive: the banner under the header pushes the page down; Dismiss
    st.set_page(4);
    st.set_security(2);
    st.set_domain("login.cafefree.example".into());
    let top = s.ui.get_page_top_px();
    st.set_captive_ssid("Cafe Free".into());
    let px = s.shoot_at("br2-captive", GESTURES);
    assert_eq!(s.ui.get_page_top_px(), top + 60 * 3);
    assert_eq!(
        at(&px, 100.0, 48.0 + 30.0) >> 8,
        0x13233d >> 8,
        "the banner's blue"
    );
    s.take_acts();
    s.tap(360.0 - 8.0 - 24.0, 48.0 + 30.0);
    assert_eq!(s.take_acts(), [("captive-dismiss".to_string(), 0)]);
}
