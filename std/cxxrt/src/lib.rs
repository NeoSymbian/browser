// SPDX-License-Identifier: Apache-2.0
//! The C++ runtime for ARMv7 C++ code linked into a NeoSymbian EXE (docs browser-js.md): GCC's thread-safe static
//! initialisation (`__cxa_guard_*`), emulated TLS for `thread_local` (`__emutls_get_address`), over P.I.P.S.'s
//! pthreads, libstdc++'s `std::__throw_*` helpers as fatal errors (C++ here is built without exceptions; libstdc++'s
//! own `.a` is never linked whole, only its self-contained tree/list/hashtable pieces, build.rs) and the stack
//! protector's canary, seeded from the kernel's generator. The rest of the C++ ABI comes from build's start-up code (`operator new`/`delete`,
//! `__cxa_pure_virtual`, `__aeabi_atexit`). Link it by naming the crate (`use cxxrt as _;`).

use std::ffi::{CStr, c_char};

/// A C++ error that would have thrown: the message on stderr (the debug port), then abort.
#[unsafe(no_mangle)]
pub extern "C" fn cxxrt_fatal(what: *const c_char, detail: *const c_char) -> ! {
    let s = |p: *const c_char| {
        if p.is_null() {
            "".into()
        } else {
            unsafe { CStr::from_ptr(p) }.to_string_lossy()
        }
    };
    eprintln!("cxxrt: fatal C++ error: {} {}", s(what), s(detail));
    std::process::abort()
}
