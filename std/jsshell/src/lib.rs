// SPDX-License-Identifier: Apache-2.0
//! JSSHELL.EXE (docs browser/wb3-js.md, WB3): SpiderMonkey 153 (interpreter only, no Intl) through the mozjs crate on
//! NeoSymbian. `jsshell` runs the built-in suite (tests/*.js: language, async, GC stress; 4 runtimes on 4 threads;
//! a clean out-of-memory on the GC heap and on the native heap) and prints `js <name> ... ok|FAILED`;
//! `jsshell FILE.js...` runs scripts; `jsshell -e CODE` one expression.

use std::ffi::CStr;
use std::ptr::{self, NonNull};
use std::time::Instant;

use cxxrt as _;
use mozjs::context::{JSContext, RawJSContext};
use mozjs::jsapi::{CallArgs, GCOptions, GCReason, JSGCParamKey, OnNewGlobalHookOption, Value};
use mozjs::jsval::{DoubleValue, UndefinedValue};
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::wrappers2::*;
use mozjs::rust::{
    CompileOptionsWrapper, JSEngine, ParentRuntime, RealmOptions, Runtime, SIMPLE_GLOBAL_CLASS,
    evaluate_script,
};

e32::main!(main);

const PRELUDE: &str = include_str!("prelude.js");
const LANGUAGE: &[(&str, &str)] = &[
    ("types", include_str!("../tests/types.js")),
    ("objects", include_str!("../tests/objects.js")),
    ("collections", include_str!("../tests/collections.js")),
    ("text", include_str!("../tests/text.js")),
    ("async", include_str!("../tests/async.js")),
];
const GC_STRESS: &str = include_str!("../tests/gc.js");

// ---- the job queue (promise reactions): SpiderMonkey keeps the microtasks; the embedder's JobQueue says when they
// run. mozjs initialises self-hosting in Runtime::new, too late for UseInternalJobQueues, so the shell gives each
// runtime a RustJobQueue (mozjs's glue) whose runJobs drains them as SpiderMonkey's own InternalJobQueue does.

unsafe extern "C" fn jq_host_defined_data(
    cx: *mut RawJSContext,
    incumbent: mozjs::jsapi::MutableHandleObject,
    data: mozjs::jsapi::MutableHandleObject,
) -> bool {
    unsafe {
        *incumbent.ptr = mozjs::jsapi::CurrentGlobalOrNull(cx);
        *data.ptr = ptr::null_mut();
    }
    true
}

unsafe extern "C" fn jq_host_defined_global(
    cx: *mut RawJSContext,
    data: mozjs::jsapi::MutableHandleObject,
) -> bool {
    unsafe { *data.ptr = mozjs::jsapi::CurrentGlobalOrNull(cx) };
    true
}

unsafe extern "C" fn jq_run_jobs(raw: *mut RawJSContext) {
    let mut cx = unsafe { JSContext::from_ptr(NonNull::new(raw).unwrap()) };
    unsafe {
        while mozjs::jsapi::HasAnyMicroTasks(raw) {
            rooted!(&in(cx) let task = mozjs::jsapi::DequeueNextRegularMicroTask(raw));
            if task.get().is_null() {
                break; // only debugger microtasks left: none here
            }
            rooted!(&in(cx) let job = mozjs::jsapi::ToMaybeWrappedJSMicroTask(&task.get()));
            if job.get().is_null() {
                continue;
            }
            if !mozjs::jsapi::HasAnyMicroTasks(raw) {
                mozjs::jsapi::JobQueueIsEmpty(raw);
            }
            let global = mozjs::jsapi::GetExecutionGlobalFromJSMicroTask(job.get());
            if global.is_null() {
                continue;
            }
            let old = mozjs::jsapi::EnterRealm(raw, global);
            let ok = mozjs::jsapi::RunJSMicroTask(raw, job.handle().into());
            mozjs::jsapi::LeaveRealm(raw, old);
            if !ok && mozjs::jsapi::JS_IsExceptionPending(raw) {
                println!("js uncaught in a job: {}", take_exception(&mut cx));
            }
        }
    }
}

unsafe extern "C" fn jq_trace(_trc: *mut mozjs::jsapi::JSTracer, _v: *mut Value) {}

static JOB_QUEUE_TRAPS: mozjs::glue::JobQueueTraps = mozjs::glue::JobQueueTraps {
    getHostDefinedData: Some(jq_host_defined_data),
    getHostDefinedGlobal: Some(jq_host_defined_global),
    runJobs: Some(jq_run_jobs),
    traceNonGCThingMicroTask: Some(jq_trace),
};

/// A runtime with a job queue (promise reactions run by RunJobs): `parent` makes it a child runtime for another
/// thread. The queue lives as long as the process (one per runtime, a few bytes).
fn new_runtime(engine: Option<&JSEngine>, parent: Option<ParentRuntime>) -> Runtime {
    let mut rt = match (engine, parent) {
        (_, Some(p)) => unsafe { Runtime::create_with_parent(p) },
        (Some(e), None) => Runtime::new(e.handle()),
        (None, None) => unreachable!(),
    };
    unsafe {
        let queue = mozjs::glue::CreateJobQueue(&JOB_QUEUE_TRAPS);
        SetJobQueue(rt.cx(), queue as *mut mozjs::jsapi::JobQueue);
    }
    rt
}

/// The text of `cx`'s pending exception (cleared), or why there is none.
fn take_exception(cx: &mut JSContext) -> String {
    unsafe {
        if !JS_IsExceptionPending(cx) {
            return "uncatchable (no exception: out of memory or a forced termination)".into();
        }
        rooted!(&in(cx) let mut exc = UndefinedValue());
        JS_GetPendingException(cx, exc.handle_mut());
        JS_ClearPendingException(cx);
        let s = mozjs::rust::ToString(cx, exc.handle());
        match NonNull::new(s) {
            Some(s) => mozjs::conversions::jsstr_to_string(cx, s),
            None => {
                JS_ClearPendingException(cx);
                "(exception not convertible to a string)".into()
            }
        }
    }
}

unsafe extern "C" fn js_print(cx: *mut RawJSContext, argc: u32, vp: *mut Value) -> bool {
    let mut cx = unsafe { JSContext::from_ptr(NonNull::new(cx).unwrap()) };
    let args = unsafe { CallArgs::from_vp(vp, argc) };
    let mut line = String::new();
    for i in 0..argc {
        let arg = unsafe { mozjs::rust::Handle::from_raw(args.get(i)) };
        let s = unsafe { mozjs::rust::ToString(&mut cx, arg) };
        let Some(s) = NonNull::new(s) else {
            return false;
        };
        if i > 0 {
            line.push(' ');
        }
        line.push_str(&unsafe { mozjs::conversions::jsstr_to_string(&cx, s) });
    }
    println!("{line}");
    args.rval().set(UndefinedValue());
    true
}

unsafe extern "C" fn js_gc(cx: *mut RawJSContext, argc: u32, vp: *mut Value) -> bool {
    let mut cx = unsafe { JSContext::from_ptr(NonNull::new(cx).unwrap()) };
    let args = unsafe { CallArgs::from_vp(vp, argc) };
    unsafe { NonIncrementalGC(&mut cx, GCOptions::Shrink, GCReason::API) };
    args.rval().set(UndefinedValue());
    true
}

unsafe extern "C" fn js_gc_chunks(cx: *mut RawJSContext, argc: u32, vp: *mut Value) -> bool {
    let cx = unsafe { JSContext::from_ptr(NonNull::new(cx).unwrap()) };
    let args = unsafe { CallArgs::from_vp(vp, argc) };
    let n = unsafe { JS_GetGCParameter(&cx, JSGCParamKey::JSGC_TOTAL_CHUNKS) };
    args.rval().set(DoubleValue(f64::from(n)));
    true
}

/// One global with the natives and the prelude: runs `script` in it, then the job queue; `check` (an expression)
/// must then be true.
fn run_script(
    rt: &mut Runtime,
    name: &str,
    script: &str,
    check: Option<&str>,
) -> Result<String, String> {
    let cx = rt.cx();
    let options = RealmOptions::default();
    rooted!(&in(cx) let global = unsafe {
        JS_NewGlobalObject(cx, &SIMPLE_GLOBAL_CLASS, ptr::null_mut(), OnNewGlobalHookOption::FireOnNewGlobalHook,
            &*options)
    });
    if global.get().is_null() {
        // no realm to read the exception in (JS_GetPendingException would fault): drop it
        unsafe { JS_ClearPendingException(cx) };
        return Err("no global (out of memory)".into());
    }
    let mut realm = AutoRealm::new_from_handle(cx, global.handle());
    let (global, cx) = realm.global_and_reborrow();
    let natives: [(&CStr, mozjs::jsapi::JSNative, u32); 3] = [
        (c"print", Some(js_print), 1),
        (c"gc", Some(js_gc), 0),
        (c"gcChunks", Some(js_gc_chunks), 0),
    ];
    for (n, f, arity) in natives {
        if unsafe { JS_DefineFunction(cx, global, n.as_ptr(), f, arity, 0) }.is_null() {
            return Err(format!("defining {n:?}"));
        }
    }
    let eval = |cx: &mut JSContext, file: &CStr, src: &str| -> Result<Value, String> {
        rooted!(&in(cx) let mut rval = UndefinedValue());
        let opts = CompileOptionsWrapper::new(cx, file.to_owned(), 1);
        match evaluate_script(cx, global, src, rval.handle_mut(), opts) {
            Ok(()) => Ok(rval.get()),
            Err(()) => Err(take_exception(cx)),
        }
    };
    eval(cx, c"prelude.js", PRELUDE).map_err(|e| format!("prelude: {e}"))?;
    let file = std::ffi::CString::new(format!("{name}.js")).unwrap();
    let t = Instant::now();
    eval(cx, &file, script)?;
    unsafe { RunJobs(cx) };
    if unsafe { JS_IsExceptionPending(cx) } {
        return Err(format!("in a job: {}", take_exception(cx)));
    }
    let ms = t.elapsed().as_millis();
    if let Some(check) = check {
        let v = eval(cx, c"check.js", check)?;
        if !(v.is_boolean() && v.to_boolean()) {
            let why = eval(
                cx,
                c"check.js",
                "String(typeof asyncError === 'undefined' ? '' : asyncError)",
            )
            .ok()
            .filter(|v| v.is_string())
            .map(|_| "see asyncError")
            .unwrap_or("");
            return Err(format!("check `{check}` false {why}"));
        }
    }
    Ok(format!("{ms} ms"))
}

/// The language set on one runtime: Err(first failure)
fn language(rt: &mut Runtime) -> Result<(), String> {
    for (name, src) in LANGUAGE {
        let check = (*name == "async").then_some("asyncDone");
        run_script(rt, name, src, check).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

fn report(name: &str, r: Result<String, String>, ok: &mut u32, failed: &mut u32) {
    match r {
        Ok(d) => {
            *ok += 1;
            println!("js {name} ... ok: {d}");
        }
        Err(e) => {
            *failed += 1;
            println!("js {name} ... FAILED: {e}");
        }
    }
}

/// A runtime of its own (on a thread of its own: SpiderMonkey allows one context per thread) with the GC heap capped
/// at `max_bytes`: `script` must fail with an out-of-memory error, and the runtime must work again afterwards.
fn oom(parent: ParentRuntime, max_bytes: u32, script: &'static str) -> Result<String, String> {
    std::thread::Builder::new()
        .name("js-oom".into())
        .stack_size(2 << 20)
        .spawn(move || {
            let mut rt = new_runtime(None, Some(parent));
            unsafe { JS_SetGCParameter(rt.cx(), JSGCParamKey::JSGC_MAX_BYTES, max_bytes) };
            let err = match run_script(&mut rt, "oom", script, None) {
                Ok(_) => return Err("the allocation never failed".to_string()),
                Err(e) => e,
            };
            if !err.contains("out of memory")
                && !err.contains("uncatchable")
                && !err.contains("allocation size overflow")
            {
                return Err(format!("failed, but not for memory: {err}"));
            }
            // lift the cap and give the memory back before checking the runtime still works
            unsafe {
                JS_SetGCParameter(rt.cx(), JSGCParamKey::JSGC_MAX_BYTES, u32::MAX);
                NonIncrementalGC(rt.cx(), GCOptions::Shrink, GCReason::API);
            }
            run_script(
                &mut rt,
                "after-oom",
                "assertEq([1, 2, 3].map((v) => v * 2).join(), '2,4,6');",
                None,
            )
            .map_err(|e| format!("not usable after the OOM: {e}"))?;
            Ok(format!("{err}; usable afterwards"))
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|_| Err("panicked".into()))
}

fn suite(engine: &JSEngine) -> i32 {
    let (mut ok, mut failed) = (0, 0);
    let mut rt = new_runtime(Some(engine), None);
    for (name, src) in LANGUAGE {
        let check = (*name == "async").then_some("asyncDone");
        report(
            name,
            run_script(&mut rt, name, src, check),
            &mut ok,
            &mut failed,
        );
    }

    // GC stress, then a shrinking GC: the chunks the churn took must go back
    let r = run_script(&mut rt, "gc", &format!("{GC_STRESS}\ngc(); gc();"), None).and_then(|t| {
        let cx = rt.cx();
        let chunks = unsafe { JS_GetGCParameter(cx, JSGCParamKey::JSGC_TOTAL_CHUNKS) };
        Ok(format!("{t}; {chunks} chunks after a shrinking GC"))
    });
    report("gc_stress", r, &mut ok, &mut failed);

    // 4 runtimes on 4 threads at once, each running the language set
    let t = Instant::now();
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let parent = rt.prepare_for_new_child();
            std::thread::Builder::new()
                .name(format!("js{i}"))
                .stack_size(2 << 20)
                .spawn(move || {
                    let mut child = new_runtime(None, Some(parent));
                    language(&mut child)
                })
                .expect("thread")
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().unwrap_or(Err("panicked".into())))
        .collect();
    let r = match results.iter().position(Result::is_err) {
        None => Ok(format!(
            "4 runtimes x {} scripts in {} ms",
            LANGUAGE.len(),
            t.elapsed().as_millis()
        )),
        Some(i) => Err(format!("thread {i}: {}", results[i].clone().unwrap_err())),
    };
    report("threads", r, &mut ok, &mut failed);

    // a clean out-of-memory: the GC heap capped at 32 MB, then plain objects without end
    let r = oom(
        rt.prepare_for_new_child(),
        32 << 20,
        "var hog = []; for (;;) hog.push({ a: hog.length, b: [1, 2] });",
    );
    report("oom_gc_heap", r, &mut ok, &mut failed);
    // and the native heap (heap= in exes.txt): 4 MiB buffers without end
    let r = oom(
        rt.prepare_for_new_child(),
        u32::MAX,
        "var bufs = []; for (;;) bufs.push(new Uint8Array(4 << 20));",
    );
    report("oom_native_heap", r, &mut ok, &mut failed);

    println!("jssuite: {ok} ok, {failed} failed");
    if failed == 0 { 0 } else { 1 }
}

fn main() -> i32 {
    let engine = match JSEngine::init() {
        Ok(e) => e,
        Err(e) => {
            println!("js engine ... FAILED: {e:?}");
            return 1;
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => suite(&engine),
        Some("-e") => {
            let mut rt = new_runtime(Some(&engine), None);
            let code = args[1..].join(" ");
            let r = run_script(
                &mut rt,
                "eval",
                &format!("print(eval({}))", serde_like_quote(&code)),
                None,
            );
            if let Err(e) = &r {
                println!("js error: {e}");
            }
            r.is_err() as i32
        }
        Some(_) => {
            let mut rt = new_runtime(Some(&engine), None);
            let (mut ok, mut failed) = (0, 0);
            for path in &args {
                let r = std::fs::read_to_string(path)
                    .map_err(|e| format!("{path}: {e}"))
                    .and_then(|src| run_script(&mut rt, path, &src, None));
                report(path, r, &mut ok, &mut failed);
            }
            (failed > 0) as i32
        }
    }
}

/// `code` as a JS string literal
fn serde_like_quote(code: &str) -> String {
    let mut out = String::from("\"");
    for ch in code.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
