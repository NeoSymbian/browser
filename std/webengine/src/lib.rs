// SPDX-License-Identifier: Apache-2.0
//! NEOWEBE.EXE (docs browser.md WB4): Servo on NeoSymbian, rendered with SWGL.
//!
//! `neowebe --shot URL OUT.png [--size WxH] [--dpr D] [--timeout SECS]` loads one page headless, waits until it is
//! loaded and painted, writes the frame as a PNG and prints the time, frames and memory (WB4b).

mod random;
mod swgl;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use cxxrt as _;
use dpi::PhysicalSize;
use euclid::Scale;
use servo::{
    ConsoleLogLevel, EventLoopWaker, LoadStatus, PrefValue, Preferences, RenderingContext,
    ServoBuilder, WebView, WebViewBuilder, WebViewDelegate,
};

e32::main!(main);

/// Interpreter-only JavaScript: the engine is built without the JIT (WB3).
const INTERPRETER_PREFS: &[(&str, bool)] = &[
    ("js_disable_jit", true),
    ("js_baseline_interpreter_enabled", false),
    ("js_baseline_jit_enabled", false),
    ("js_ion_enabled", false),
    ("js_native_regex_enabled", false),
    ("js_wasm_enabled", false),
    ("js_asmjs_enabled", false),
    // rustls' bundled roots: there is no system certificate store
    ("network_use_webpki_roots", true),
];

/// Wakes the embedder loop when Servo has work for it.
#[derive(Clone, Default)]
struct Waker(Arc<(Mutex<bool>, Condvar)>);

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        let (woken, cv) = &*self.0;
        *woken.lock().unwrap() = true;
        cv.notify_one();
    }
}

impl Waker {
    fn wait(&self, timeout: Duration) {
        let (woken, cv) = &*self.0;
        let guard = woken.lock().unwrap();
        let (mut guard, _) = cv.wait_timeout_while(guard, timeout, |w| !*w).unwrap();
        *guard = false;
    }
}

/// Servo's notifications to the embedder.
#[derive(Default)]
struct State {
    complete: Cell<bool>,
    frame_ready: Cell<bool>,
    crashed: Cell<bool>,
}

impl WebViewDelegate for State {
    fn notify_load_status_changed(&self, _: WebView, status: LoadStatus) {
        println!("neowebe: load {status:?}");
        if matches!(status, LoadStatus::Complete) {
            self.complete.set(true);
        }
    }

    fn notify_new_frame_ready(&self, _: WebView) {
        self.frame_ready.set(true);
    }

    fn show_console_message(&self, _: WebView, level: ConsoleLogLevel, message: String) {
        println!("neowebe: console {level:?} {message}");
    }

    fn notify_crashed(&self, _: WebView, reason: String, _backtrace: Option<String>) {
        println!("neowebe: crashed: {reason}");
        self.crashed.set(true);
    }
}

struct Shot {
    url: url::Url,
    out: String,
    size: PhysicalSize<u32>,
    dpr: f32,
    timeout: Duration,
}

fn parse(args: &[String]) -> Result<Shot, String> {
    let mut it = args.iter();
    let (Some(flag), Some(url), Some(out)) = (it.next(), it.next(), it.next()) else {
        return Err(
            "usage: neowebe --shot URL OUT.png [--size WxH] [--dpr D] [--timeout SECS]".into(),
        );
    };
    if flag != "--shot" {
        return Err(format!("{flag}: only --shot so far"));
    }
    let mut shot = Shot {
        url: url::Url::parse(url).map_err(|e| format!("{url}: {e}"))?,
        out: out.clone(),
        size: PhysicalSize::new(1080, 2037),
        dpr: 2.75,
        timeout: Duration::from_secs(120),
    };
    while let Some(flag) = it.next() {
        let value = it.next().ok_or(format!("{flag}: no value"))?;
        match flag.as_str() {
            "--size" => {
                let (w, h) = value.split_once('x').ok_or("--size WxH")?;
                shot.size = PhysicalSize::new(
                    w.parse().map_err(|_| "--size WxH")?,
                    h.parse().map_err(|_| "--size WxH")?,
                );
            }
            "--dpr" => shot.dpr = value.parse().map_err(|_| "--dpr D")?,
            "--timeout" => {
                shot.timeout = Duration::from_secs(value.parse().map_err(|_| "--timeout SECS")?)
            }
            _ => return Err(format!("{flag}: unknown")),
        }
    }
    Ok(shot)
}

fn shot(s: Shot) -> Result<String, String> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| "a rustls crypto provider is installed already")?;
    let mut prefs = Preferences::default();
    for (name, value) in INTERPRETER_PREFS {
        prefs.set_value(name, PrefValue::Bool(*value));
    }
    let ctx: Rc<dyn RenderingContext> = Rc::new(swgl::SwglRenderingContext::new(s.size));
    let _ = ctx.make_current();
    let waker = Waker::default();
    let started = Instant::now();
    let servo = ServoBuilder::default()
        .preferences(prefs)
        .event_loop_waker(Box::new(waker.clone()))
        .build();
    let state = Rc::new(State::default());
    let webview = WebViewBuilder::new(&servo, ctx.clone())
        .url(s.url.clone())
        .hidpi_scale_factor(Scale::new(s.dpr))
        .delegate(state.clone())
        .build();
    let mut frames = 0u32;
    // after the load completes, keep painting for a moment: images and fonts arrive in later frames
    let mut settled: Option<Instant> = None;
    loop {
        servo.spin_event_loop();
        if state.frame_ready.replace(false) {
            webview.paint();
            ctx.present();
            frames += 1;
        }
        if state.crashed.get() {
            return Err("Servo crashed".into());
        }
        if state.complete.get() && frames > 0 {
            let t = *settled.get_or_insert_with(Instant::now);
            if t.elapsed() > Duration::from_millis(1500) {
                break;
            }
        }
        if started.elapsed() > s.timeout {
            return Err(format!("timeout after {:?}, {frames} frames", s.timeout));
        }
        waker.wait(Duration::from_millis(20));
    }
    let ms = started.elapsed().as_millis();
    let image = ctx
        .read_to_image(servo::DeviceIntRect::from_size(ctx.size2d().to_i32()))
        .ok_or("no pixels")?;
    image.save(&s.out).map_err(|e| format!("{}: {e}", s.out))?;
    Ok(format!("{ms} ms, {frames} frames"))
}

fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let s = match parse(&args) {
        Ok(s) => s,
        Err(e) => {
            println!("neowebe: {e}");
            return 64;
        }
    };
    let what = format!("{} -> {}", s.url, s.out);
    match shot(s) {
        Ok(stats) => {
            println!("neowebe: shot {what} ok: {stats}");
            0
        }
        Err(e) => {
            println!("neowebe: shot {what} FAILED: {e}");
            1
        }
    }
}
