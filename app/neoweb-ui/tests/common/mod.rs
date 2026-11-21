// SPDX-License-Identifier: GPL-3.0-only
//! The snapshot harness (as NeoSymbian/phone's): one software-rendered `BrowserApp` per test binary (Slint's platform
//! is per process), drawn at the Pixel's dp = 3 into `snapshots/<name>.png` at the repo root, for side-by-side review
//! with the canvas's `br2-*` boards (360 dp wide): the app area in Gestures mode (679 dp) or Buttons mode (639 dp).
#![allow(dead_code)]

use neoweb_ui::{BrowserApp, BrowserState, ShortcutTile, Theme};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType, TargetPixel,
};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

pub const W: u32 = 1080;
pub const DP: f32 = 3.0;
pub const GESTURES: u32 = 679;
pub const BUTTONS: u32 = 639;

struct Host(Rc<MinimalSoftwareWindow>);

thread_local! {
    static NOW: std::cell::Cell<core::time::Duration> = const { std::cell::Cell::new(core::time::Duration::ZERO) };
}

impl Platform for Host {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
    fn duration_since_start(&self) -> core::time::Duration {
        NOW.get()
    }
}

#[derive(Clone, Copy, Default)]
struct Px(u32);

impl TargetPixel for Px {
    fn blend(&mut self, c: PremultipliedRgbaColor) {
        let a = 255 - u32::from(c.alpha);
        let ch = |s: u32, v: u8| (u32::from(v) + ((self.0 >> s) & 0xff) * a / 255).min(255);
        self.0 = ch(16, c.red) << 16 | ch(8, c.green) << 8 | ch(0, c.blue);
    }
    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Px(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
    }
}

pub struct Shot {
    window: Rc<MinimalSoftwareWindow>,
    pub ui: BrowserApp,
    /// every `act(action, value)` the screens sent, oldest first
    pub acts: Rc<RefCell<Vec<(String, i32)>>>,
}

/// The app at the Pixel's width with the board's shortcuts; call once per test binary.
pub fn app() -> Shot {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Host(window.clone())))
        .expect("one platform per process");
    let ui = BrowserApp::new().expect("BrowserApp");
    ui.global::<Theme>().set_dp(DP);
    window.set_size(slint::PhysicalSize::new(W, GESTURES * 3));
    ui.show().expect("show");
    let acts = Rc::new(RefCell::new(Vec::new()));
    let a = acts.clone();
    let s = ui.global::<BrowserState>();
    s.on_act(move |k: SharedString, v| a.borrow_mut().push((k.into(), v)));
    let tiles: Vec<ShortcutTile> = neoweb_core::start::SHORTCUTS
        .iter()
        .map(|t| ShortcutTile {
            letters: t.letters.into(),
            label: t.label.into(),
            color: slint::Color::from_argb_encoded(0xff00_0000 | t.color),
        })
        .collect();
    s.set_shortcuts(ModelRc::from(Rc::new(VecModel::from(tiles))));
    Shot { window, ui, acts }
}

pub fn strings(v: &[&str]) -> ModelRc<SharedString> {
    ModelRc::from(Rc::new(VecModel::from(
        v.iter().map(|s| SharedString::from(*s)).collect::<Vec<_>>(),
    )))
}

impl Shot {
    pub fn state(&self) -> BrowserState<'_> {
        self.ui.global::<BrowserState>()
    }

    /// The acts sent since the last call.
    pub fn take_acts(&self) -> Vec<(String, i32)> {
        std::mem::take(&mut *self.acts.borrow_mut())
    }

    /// A tap at (x, y) dp of the app's surface.
    pub fn tap(&self, x: f32, y: f32) {
        let position = LogicalPosition::new(x * DP, y * DP);
        let w = self.window.window();
        w.dispatch_event(WindowEvent::PointerMoved { position });
        w.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        w.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        w.dispatch_event(WindowEvent::PointerExited);
        self.settle();
    }

    /// A hold at (x, y) dp: pressed past the kit's 500 ms long-press, then released.
    pub fn long_press(&self, x: f32, y: f32) {
        let position = LogicalPosition::new(x * DP, y * DP);
        let w = self.window.window();
        w.dispatch_event(WindowEvent::PointerMoved { position });
        w.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        self.settle();
        w.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        w.dispatch_event(WindowEvent::PointerExited);
        self.settle();
    }

    pub fn settle(&self) {
        NOW.set(NOW.get() + core::time::Duration::from_secs(1));
        slint::platform::update_timers_and_animations();
    }

    /// Draw the app `h_dp` high; the pixels as 0xRRGGBB.
    pub fn draw_at(&self, h_dp: u32) -> Vec<u32> {
        let h = h_dp * 3;
        if self.window.size().height != h {
            self.window.set_size(slint::PhysicalSize::new(W, h));
        }
        self.settle();
        self.window.request_redraw();
        let mut px = vec![Px(0); (W * h) as usize];
        self.window.draw_if_needed(|r| {
            r.render(&mut px, W as usize);
        });
        px.into_iter().map(|p| p.0).collect()
    }

    /// Draw the app `h_dp` high and save `snapshots/<name>.png`; returns the pixels for checks.
    pub fn shoot_at(&self, name: &str, h_dp: u32) -> Vec<u32> {
        let px = self.draw_at(h_dp);
        save(name, &px, h_dp * 3);
        px
    }
}

/// Save 0xRRGGBB pixels, `W` wide and `h` high, as `snapshots/<name>.png`.
pub fn save(name: &str, px: &[u32], h: u32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../snapshots");
    std::fs::create_dir_all(&root).unwrap();
    let file = std::fs::File::create(root.join(format!("{name}.png"))).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    let rgb: Vec<u8> = px
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
        .collect();
    w.write_image_data(&rgb).unwrap();
}

/// The pixel at (x, y) dp.
pub fn at(px: &[u32], x: f32, y: f32) -> u32 {
    px[((y * DP) as u32 * W + (x * DP) as u32) as usize]
}
