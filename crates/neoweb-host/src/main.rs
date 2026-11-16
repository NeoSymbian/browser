// SPDX-License-Identifier: Apache-2.0
//! neoweb-host: a headless Servo embedder for WB0. It runs a site workflow (or one URL) with software GL and writes
//! `steps.csv`, PNGs, memory reports and `console.log` into `--out`.
//!
//! neoweb-host (--workflow FILE | URL) [--gl mesa] [--size 1080x2160] [--dpr 2.75] [--out DIR]
//!             [--pref NAME=VALUE]... [--jit] [--soak SECS] [--timeout SECS] [--fb /dev/fb0]
//!
//! Exit codes: 0 all required steps passed, 1 a step failed, 2 timeout, 3 Servo crashed, 64 bad arguments.

mod delegate;
mod exec;
mod out;

use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use dpi::PhysicalSize;
use euclid::Scale;
use servo::{
    EventLoopWaker, PrefValue, Preferences, RenderingContext, ServoBuilder,
    SoftwareRenderingContext, WebViewBuilder,
};
use wb0_workflow::{Action, Step, Workflow};

use crate::delegate::State;
use crate::exec::{Cx, Exec, Poll};
use crate::out::{Framebuffer, Out};

/// Interpreter-only JavaScript, as on the phone; `--jit` skips these (JIT comparison build only).
const INTERPRETER_PREFS: &[(&str, bool)] = &[
    ("js_disable_jit", true),
    ("js_baseline_interpreter_enabled", false),
    ("js_baseline_jit_enabled", false),
    ("js_ion_enabled", false),
    ("js_native_regex_enabled", false),
    ("js_wasm_enabled", false),
    ("js_asmjs_enabled", false),
];

struct Args {
    workflow: Workflow,
    gl: String,
    size: PhysicalSize<u32>,
    dpr: f32,
    out: PathBuf,
    prefs: Vec<(String, String)>,
    jit: bool,
    soak: Option<Duration>,
    timeout: Duration,
    fb: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        workflow: Workflow {
            name: String::new(),
            url: String::new(),
            steps: vec![],
        },
        gl: "mesa".into(),
        size: PhysicalSize::new(1080, 2160),
        dpr: 2.75,
        out: "out".into(),
        prefs: vec![],
        jit: false,
        soak: None,
        timeout: Duration::from_secs(900),
        fb: None,
    };
    let mut it = std::env::args().skip(1);
    let secs = |v: String| {
        v.parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|e| format!("{v}: {e}"))
    };
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--workflow" => a.workflow = wb0_workflow::load(std::path::Path::new(&value()?))?,
            "--gl" => a.gl = value()?,
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or(format!("--size {v}: want WxH"))?;
                a.size = PhysicalSize::new(
                    w.parse().map_err(|_| "bad width")?,
                    h.parse().map_err(|_| "bad height")?,
                );
            }
            "--dpr" => a.dpr = value()?.parse().map_err(|_| "bad --dpr")?,
            "--out" => a.out = value()?.into(),
            "--pref" => {
                let v = value()?;
                let (k, val) = v.split_once('=').unwrap_or((&v, "true"));
                a.prefs.push((k.to_string(), val.to_string()));
            }
            "--jit" => a.jit = true,
            "--soak" => a.soak = Some(secs(value()?)?),
            "--timeout" => a.timeout = secs(value()?)?,
            "--fb" => a.fb = Some(value()?.into()),
            url if !url.starts_with('-') && a.workflow.url.is_empty() => {
                let step = |action| Step {
                    action,
                    optional: false,
                    timeout_ms: None,
                };
                a.workflow = Workflow {
                    name: "url".into(),
                    url: url.into(),
                    steps: vec![
                        step(Action::WaitLoad),
                        step(Action::Shot {
                            name: "page".into(),
                        }),
                        step(Action::Mem {
                            name: "page".into(),
                        }),
                    ],
                };
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if a.workflow.url.is_empty() {
        return Err("give a URL or --workflow FILE".into());
    }
    Ok(a)
}

/// Wakes the event loop when Servo has work for the embedder thread.
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

fn rendering_context(
    gl: &str,
    size: PhysicalSize<u32>,
) -> Result<Rc<dyn RenderingContext>, String> {
    match gl {
        "mesa" => Ok(Rc::new(
            SoftwareRenderingContext::new(size).map_err(|e| format!("Mesa context: {e:?}"))?,
        )),
        other => Err(format!("--gl {other}: this build supports mesa")),
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("neoweb-host: {e}");
            return ExitCode::from(64);
        }
    };
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install the rustls crypto provider");

    let mut prefs = Preferences::default();
    if !args.jit {
        for (name, value) in INTERPRETER_PREFS {
            prefs.set_value(name, PrefValue::Bool(*value));
        }
    }
    for (name, value) in &args.prefs {
        prefs.set_value(name, PrefValue::from_booleanish_str(value));
    }

    let ctx = match rendering_context(&args.gl, args.size) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("neoweb-host: {e}");
            return ExitCode::from(64);
        }
    };
    let _ = ctx.make_current();
    let mut out = match Out::new(&args.out) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("neoweb-host: {}: {e}", args.out.display());
            return ExitCode::from(64);
        }
    };
    let mut fb = args.fb.as_deref().and_then(|dev| {
        Framebuffer::open(dev)
            .map_err(|e| eprintln!("neoweb-host: {}: {e}", dev.display()))
            .ok()
    });

    let waker = Waker::default();
    let servo = ServoBuilder::default()
        .preferences(prefs)
        .event_loop_waker(Box::new(waker.clone()))
        .build();
    servo.setup_logging();
    let state = Rc::new(State::new(
        std::fs::File::create(out.path("console.log")).ok(),
    ));
    let url = match url::Url::parse(&args.workflow.url) {
        Ok(url) => url,
        Err(e) => {
            eprintln!("neoweb-host: {}: {e}", args.workflow.url);
            return ExitCode::from(64);
        }
    };
    let webview = WebViewBuilder::new(&servo, ctx.clone())
        .url(url)
        .hidpi_scale_factor(Scale::new(args.dpr))
        .delegate(state.clone())
        .build();

    eprintln!(
        "neoweb-host: {} on {} at {}x{} DPR {}",
        args.workflow.name, args.gl, args.size.width, args.size.height, args.dpr
    );
    let mut exec = Exec::new(args.workflow, args.soak);
    let started = Instant::now();
    let mut last_blit = started;
    let code = loop {
        servo.spin_event_loop();
        if state.frame_ready.replace(false) {
            webview.paint();
            ctx.present();
            let now = Instant::now();
            state.painted(now);
            if let Some(fb) = fb
                .as_mut()
                .filter(|_| now.duration_since(last_blit) >= Duration::from_secs(1))
            {
                last_blit = now;
                if let Some(image) =
                    ctx.read_to_image(servo::DeviceIntRect::from_size(ctx.size2d().to_i32()))
                {
                    let _ = fb.blit(&image);
                }
            }
        }
        if let Some(reason) = state.crashed.borrow().as_ref() {
            eprintln!("neoweb-host: Servo crashed: {reason}");
            break 3;
        }
        let cx = Cx {
            servo: &servo,
            webview: &webview,
            ctx: &ctx,
            state: &state,
        };
        if let Poll::Done(code) = exec.poll(&cx, &mut out) {
            break code;
        }
        if started.elapsed() > args.timeout {
            eprintln!("neoweb-host: timeout after {:?}", args.timeout);
            break 2;
        }
        waker.wait(Duration::from_millis(20));
    };
    eprintln!(
        "neoweb-host: exit {code} after {} ms, {} frames",
        state.ms(),
        state.frames.get()
    );
    ExitCode::from(code)
}
