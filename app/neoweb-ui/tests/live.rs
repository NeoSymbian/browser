// SPDX-License-Identifier: Apache-2.0
mod common;

use common::{BUTTONS, GESTURES, W};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

#[test]
fn engine_frame_fills_page_and_crash_can_reload() {
    let shot = common::app();
    let state = shot.state();
    state.set_page(4);
    state.set_loading(true);
    for height_dp in [GESTURES, BUTTONS] {
        let page_height = (height_dp - 2 * 48) * 3;
        let mut frame = SharedPixelBuffer::<Rgba8Pixel>::new(W, page_height);
        let (upper, lower) = frame
            .make_mut_slice()
            .split_at_mut((W * (page_height / 2)) as usize);
        upper.fill(Rgba8Pixel::new(255, 0, 0, 255));
        lower.fill(Rgba8Pixel::new(0, 0, 255, 255));
        state.set_frame(Image::from_rgba8(frame));
        let pixels = shot.draw_at(height_dp);
        let top = shot.ui.get_page_top_px() as usize;
        let bottom = top + shot.ui.get_page_height_px() as usize;
        assert_eq!(shot.ui.get_page_width_px(), W as i32);
        assert_eq!(top, 48 * 3);
        assert_eq!(bottom, (height_dp as usize - 48) * 3);
        let row = |y: usize| &pixels[y * W as usize..(y + 1) * W as usize];
        assert!(row(top).iter().all(|&p| p == 0xff0000));
        assert!(row(bottom - 1).iter().all(|&p| p == 0x0000ff));
        assert_ne!(row(top - 1)[0], 0xff0000);
        assert_ne!(row(bottom)[0], 0x0000ff);
    }
    assert!(shot.take_acts().is_empty(), "live loads have no fake ticks");

    state.set_loading(false);
    state.set_page(5);
    shot.draw_at(BUTTONS);
    shot.tap(180.0, BUTTONS as f32 / 2.0 + 20.0);
    assert_eq!(shot.take_acts(), [("reload".to_string(), 0)]);
}
