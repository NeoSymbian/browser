// SPDX-License-Identifier: Apache-2.0
//! JSSHELL.EXE (docs browser-js.md, WB3): SpiderMonkey (interpreter only, no Intl) through the mozjs crate.
//! First step: one script evaluated, the result printed.

use std::ptr;

use cxxrt as _;
use mozjs::jsapi::OnNewGlobalHookOption;
use mozjs::jsval::UndefinedValue;
use mozjs::rooted;
use mozjs::rust::wrappers2::*;
use mozjs::rust::{
    CompileOptionsWrapper, JSEngine, RealmOptions, Runtime, SIMPLE_GLOBAL_CLASS, evaluate_script,
};

e32::main!(main);

fn main() -> i32 {
    let engine = JSEngine::init().expect("JS engine");
    println!("js engine ok");
    let mut rt = Runtime::new(engine.handle());
    println!("js runtime ok");
    // diagnostics (WB3 bring-up): the stack base the engine is given, and what InitSelfHostedCode left pending
    unsafe extern "C" {
        fn MozSymbianStackBase() -> *mut u8;
    }
    let here = 0u32;
    println!(
        "js stack base {:p}, sp ~{:p}",
        unsafe { MozSymbianStackBase() },
        &here as *const u32
    );
    if unsafe { JS_IsExceptionPending(rt.cx()) } {
        rooted!(&in(rt.cx()) let mut exc = UndefinedValue());
        unsafe { JS_GetPendingException(rt.cx(), exc.handle_mut()) };
        let s = unsafe { mozjs::rust::ToString(rt.cx(), exc.handle()) };
        let text = std::ptr::NonNull::new(s)
            .map(|s| unsafe { mozjs::conversions::jsstr_to_string(rt.cx(), s) })
            .unwrap_or_else(|| "(not a string)".into());
        println!("js self-hosting left an exception: {text}");
    } else {
        println!("js no exception pending");
    }
    let options = RealmOptions::default();
    rooted!(&in(rt.cx()) let global = unsafe {
        JS_NewGlobalObject(rt.cx(), &SIMPLE_GLOBAL_CLASS, ptr::null_mut(),
            OnNewGlobalHookOption::FireOnNewGlobalHook, &*options)
    });
    rooted!(&in(rt.cx()) let mut rval = UndefinedValue());
    let opts = CompileOptionsWrapper::new(rt.cx_no_gc(), c"hello.js".to_owned(), 1);
    let src = "let a = [3, 1, 2].sort(); JSON.stringify({sum: a.reduce((x, y) => x + y), a}).length + 40 - 25";
    match evaluate_script(rt.cx(), global.handle(), src, rval.handle_mut(), opts) {
        Ok(()) if rval.get().is_int32() => {
            println!("js hello ... ok: {}", rval.get().to_int32());
            0
        }
        _ => {
            println!("js hello ... FAILED");
            1
        }
    }
}
