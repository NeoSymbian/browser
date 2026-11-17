// SPDX-License-Identifier: Apache-2.0
//! PIPSPROB.EXE (docs browser-std.md, browser.md WB2b): runs the C checks of pipsprobe.cpp (threads, locks, clocks,
//! files, env, memory, pipes, sockets on P.I.P.S.) and prints the C layout table the Rust libc port needs, all over
//! the debug port: `pips <check> ... ok|FAILED|NOTSUP: <detail>`, then `layout <name> = <value>`. It also calls libc
//! straight from Rust (malloc, getpid, write), which proves a Rust EXE imports from libc.dll.
#![no_std]

extern crate alloc;

use core::ffi::{CStr, c_char, c_void};
use e32::dprintln;

e32::main!(main);

unsafe extern "C" {
    fn pips_init() -> i32;
    fn pips_layout_count() -> i32;
    fn pips_layout_name(i: i32) -> *const c_char;
    fn pips_layout_value(i: i32) -> i32;
    fn pips_check_count() -> i32;
    fn pips_check_name(i: i32) -> *const c_char;
    fn pips_check_run(i: i32, detail: *mut c_char, len: i32) -> i32;
    // straight from libc.dll
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn getpid() -> i32;
    fn write(fd: i32, buf: *const c_void, n: usize) -> isize;
}

/// A C string from the glue (static, NUL-terminated).
fn s(p: *const c_char) -> &'static str {
    // SAFETY: the glue returns pointers into its static tables or our buffer, NUL-terminated.
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("?")
}

fn main() -> i32 {
    // SAFETY: once, before any P.I.P.S. call.
    if unsafe { pips_init() } != 0 {
        dprintln!("pipsprobe: no cleanup stack");
        return 1;
    }
    // SAFETY: plain libc calls with valid arguments.
    let (pid, wrote, heap_ok) = unsafe {
        let p = malloc(64);
        let ok = !p.is_null();
        free(p);
        let msg = b"pipsprobe: hello on fd 1\n";
        (getpid(), write(1, msg.as_ptr().cast(), msg.len()), ok)
    };
    dprintln!(
        "pipsprobe: Rust -> libc.dll: getpid {}, write(1) {}, malloc {}",
        pid,
        wrote,
        heap_ok
    );
    let (mut ok, mut failed, mut notsup) = (0, 0, 0);
    // SAFETY: the glue's table functions take indexes below their counts.
    unsafe {
        for i in 0..pips_check_count() {
            let name = s(pips_check_name(i));
            dprintln!("pips {} ...", name);
            let mut detail = [0 as c_char; 160];
            let r = pips_check_run(i, detail.as_mut_ptr(), detail.len() as i32);
            let verdict = match r {
                0 => {
                    ok += 1;
                    "ok"
                }
                2 => {
                    notsup += 1;
                    "NOTSUP"
                }
                _ => {
                    failed += 1;
                    "FAILED"
                }
            };
            dprintln!("pips {} ... {}: {}", name, verdict, s(detail.as_ptr()));
        }
        for i in 0..pips_layout_count() {
            let v = pips_layout_value(i);
            if v == -0x7fff_ffff {
                dprintln!("layout {} = absent", s(pips_layout_name(i)));
            } else {
                dprintln!("layout {} = {}", s(pips_layout_name(i)), v);
            }
        }
    }
    dprintln!(
        "pipsprobe: {} ok, {} failed, {} not supported",
        ok,
        failed,
        notsup
    );
    failed
}
