// SPDX-License-Identifier: Apache-2.0
//! CXXSMOKE.EXE (docs browser/wb3-js.md, WB3a): ARMv7 C++ from the SDK (tools/symbian-sdk.sh) linked into a Rust std
//! EXE, with what SpiderMonkey needs from the C++ runtime: a static initialised once under two threads,
//! `thread_local` per thread, 64-bit atomics, NEON, libstdc++ containers. Prints `cxx <name> ... ok|FAILED`.

use cxxrt as _;
use std::ffi::{CStr, c_char, c_int};

e32::main!(main);

unsafe extern "C" {
    fn smoke_guard() -> c_int;
    fn smoke_guard_inits() -> c_int;
    fn smoke_tls(add: c_int) -> c_int;
    fn smoke_atomic64(add: u64) -> u64;
    fn smoke_neon() -> f32;
    fn smoke_stl(out: *mut c_char, len: c_int) -> c_int;
}

fn check(name: &str, r: Result<String, String>) -> bool {
    match &r {
        Ok(d) => println!("cxx {name} ... ok: {d}"),
        Err(d) => println!("cxx {name} ... FAILED: {d}"),
    }
    r.is_ok()
}

fn guard() -> Result<String, String> {
    let t: Vec<_> = (0..2)
        .map(|_| std::thread::spawn(|| unsafe { smoke_guard() }))
        .collect();
    let vals: Vec<c_int> = t.into_iter().map(|h| h.join().unwrap()).collect();
    let inits = unsafe { smoke_guard_inits() };
    if vals == [42, 42] && inits == 1 {
        Ok("2 threads, 1 init".into())
    } else {
        Err(format!("{vals:?}, {inits} inits"))
    }
}

fn tls() -> Result<String, String> {
    let main = unsafe { smoke_tls(1) };
    let others: Vec<c_int> = (1..=3)
        .map(|i| std::thread::spawn(move || unsafe { smoke_tls(i * 10) }))
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    let again = unsafe { smoke_tls(1) };
    // every thread starts at 7 / 0: main 8,1 then 9,2; thread i: 7+10i, 10i
    if main == 8001 && again == 9002 && others == [17010, 27020, 37030] {
        Ok(format!("main {main} {again}, threads {others:?}"))
    } else {
        Err(format!("main {main} {again}, threads {others:?}"))
    }
}

fn main() -> i32 {
    let mut ok = 0;
    let mut total = 0;
    let mut run = |name, r| {
        total += 1;
        ok += check(name, r) as i32;
    };
    run("guarded_static", guard());
    run("thread_local", tls());
    let a = unsafe { smoke_atomic64(1u64 << 33) };
    run(
        "atomic64",
        if a == 1u64 << 33 {
            Ok(format!("{a:#x}"))
        } else {
            Err(format!("{a:#x}"))
        },
    );
    let n = unsafe { smoke_neon() };
    run(
        "neon",
        if n == 36.0 {
            Ok(format!("{n}"))
        } else {
            Err(format!("{n}"))
        },
    );
    let mut buf = [0 as c_char; 64];
    let r = unsafe { smoke_stl(buf.as_mut_ptr(), 64) };
    let s = unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    run(
        "stl",
        if r == 0 && s == "js1-18/2/9" {
            Ok(s)
        } else {
            Err(format!("{r} {s:?}"))
        },
    );
    println!("cxxsmoke: {ok} ok, {} failed", total - ok);
    if ok == total { 0 } else { 1 }
}
