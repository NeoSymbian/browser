// SPDX-License-Identifier: Apache-2.0
//! NEOWEBE.EXE (docs browser.md WB4): Servo on NeoSymbian, rendered with SWGL.
//!
//! `neowebe --shot URL OUT.png [--size WxH] [--dpr D] [--timeout SECS]` loads one page headless, waits until it is
//! loaded and painted, writes the frame as a PNG and prints the time, frames and memory (WB4b).
//! `neowebe --serve [--log LEVEL]` is the engine process behind NEOWEB (WB4c, `serve.rs`).

mod random;
mod serve;
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

/// The engine's private folder (its SID, 0xE0007008): profile, storage, caches.
const CONFIG_DIR: &str = "C:/Private/e0007008/profile";

/// Servo's log (the `log` crate) on the debug log: warnings and errors, or everything to `level` when NEOWEBE is
/// started with `--log debug|info|trace`.
pub(crate) struct DebugLog;

impl log::Log for DebugLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::max_level()
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            let line = format!("neowebe-log {} {}: {}", r.level(), r.target(), r.args());
            // the debug port takes under 256 bytes a line
            println!("{}", &line[..line.floor_char_boundary(240)]);
        }
    }

    fn flush(&self) {}
}

/// Wakes the embedder loop when Servo has work for it.
#[derive(Clone, Default)]
pub(crate) struct Waker(Arc<(Mutex<bool>, Condvar)>);

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
    pub(crate) fn wait(&self, timeout: Duration) {
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
    /// Also print the PNG on the debug log as base64 lines (`neowebe-png NAME N DATA`), for emulators without file access.
    dump: bool,
    /// How long without a new frame counts as finished (WebRender builds frames on its own thread: slow emulators
    /// need more).
    settle: Duration,
    /// Servo's log on the debug log, up to this level.
    log: log::LevelFilter,
}

fn parse(args: &[String]) -> Result<Shot, String> {
    let mut it = args.iter();
    let (Some(flag), Some(url), Some(out)) = (it.next(), it.next(), it.next()) else {
        return Err(
            "usage: neowebe --shot URL OUT.png [--size WxH] [--dpr D] [--timeout SECS] [--dump 1] [--log LEVEL] [--settle SECS]".into(),
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
        dump: false,
        settle: Duration::from_secs(3),
        log: log::LevelFilter::Warn,
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
            "--dump" => shot.dump = value == "1",
            "--settle" => {
                shot.settle = Duration::from_secs(value.parse().map_err(|_| "--settle SECS")?)
            }
            "--log" => {
                shot.log = value
                    .parse()
                    .map_err(|_| "--log off|error|warn|info|debug|trace")?
            }
            "--timeout" => {
                shot.timeout = Duration::from_secs(value.parse().map_err(|_| "--timeout SECS")?)
            }
            _ => return Err(format!("{flag}: unknown")),
        }
    }
    Ok(shot)
}

/// Servo with our prefs, profile folder and a SWGL context of `size`, woken through `waker`.
pub(crate) fn start_servo(
    size: PhysicalSize<u32>,
    waker: &Waker,
) -> (servo::Servo, Rc<swgl::SwglRenderingContext>) {
    // the TLS provider: only the first call installs it
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let mut prefs = Preferences::default();
    for (name, value) in INTERPRETER_PREFS {
        prefs.set_value(name, PrefValue::Bool(*value));
    }
    let ctx = Rc::new(swgl::SwglRenderingContext::new(size));
    let _ = ctx.make_current();
    // Servo's storage (and its caches) in the engine's private folder on C:, never a temporary folder
    let config_dir = std::path::PathBuf::from(CONFIG_DIR);
    let _ = std::fs::create_dir_all(&config_dir);
    let opts = servo::Opts {
        config_dir: Some(config_dir),
        ..Default::default()
    };
    let servo = ServoBuilder::default()
        .opts(opts)
        .preferences(prefs)
        .event_loop_waker(Box::new(waker.clone()))
        .build();
    (servo, ctx)
}

fn shot(s: Shot) -> Result<String, String> {
    let waker = Waker::default();
    let started = Instant::now();
    let (servo, ctx) = start_servo(s.size, &waker);
    let state = Rc::new(State::default());
    let webview = WebViewBuilder::new(&servo, ctx.clone())
        .url(s.url.clone())
        .hidpi_scale_factor(Scale::new(s.dpr))
        .delegate(state.clone())
        .build();
    let mut frames = 0u32;
    // after the load completes, keep painting until no new frame came for a while: images, fonts and script
    // changes arrive in later frames (slowly, in an emulator)
    let mut last_frame = Instant::now();
    loop {
        servo.spin_event_loop();
        if state.frame_ready.replace(false) {
            webview.paint();
            ctx.present();
            frames += 1;
            last_frame = Instant::now();
        }
        if state.crashed.get() {
            return Err("Servo crashed".into());
        }
        if state.complete.get() && frames > 0 && last_frame.elapsed() > s.settle {
            break;
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
    if s.dump {
        let png = std::fs::read(&s.out).map_err(|e| format!("{}: {e}", s.out))?;
        // the file's name tells several dumps in one log apart
        let tag = s.out.rsplit(['/', '\\']).next().unwrap_or("shot");
        // 150 characters a line: the debug port takes under 256 bytes a line
        for (n, chunk) in base64(&png).as_bytes().chunks(150).enumerate() {
            println!(
                "neowebe-png {tag} {n} {}",
                std::str::from_utf8(chunk).unwrap()
            );
        }
    }
    Ok(format!("{ms} ms, {frames} frames"))
}

/// Standard base64 with padding.
fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i <= c.len() {
                A[(n >> (18 - 6 * i)) as usize & 63] as char
            } else {
                '='
            });
        }
    }
    out
}

fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--serve") {
        let level = match args.get(1..3) {
            Some([flag, l]) if flag == "--log" => l.parse().unwrap_or(log::LevelFilter::Warn),
            _ => log::LevelFilter::Warn,
        };
        return match serve::serve(level) {
            Ok(()) => 0,
            Err(e) => {
                println!("neowebe: serve FAILED: {e}");
                1
            }
        };
    }
    let s = match parse(&args) {
        Ok(s) => s,
        Err(e) => {
            println!("neowebe: {e}");
            return 64;
        }
    };
    let _ = log::set_logger(&DebugLog);
    log::set_max_level(s.log);
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
