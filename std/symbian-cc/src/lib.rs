// SPDX-License-Identifier: Apache-2.0
//! For build scripts: a `cc::Build` that compiles C/C++ for `armv7a-symbian-eabi` with the SDK's
//! `armv7-symbian-{gcc,g++}` (tools/symbian-sdk.sh; `NEOSYMBIAN_SDK` overrides `~/.local/share/neosymbian-sdk`).
//! The wrappers carry every target flag, so cc's own defaults are off.

use std::path::PathBuf;

/// The SDK's `bin/` directory.
pub fn sdk_bin() -> PathBuf {
    let sdk = std::env::var_os("NEOSYMBIAN_SDK")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("HOME"))
                .join(".local/share/neosymbian-sdk")
        });
    println!("cargo:rerun-if-env-changed=NEOSYMBIAN_SDK");
    // a new SDK (flags, compat headers) rebuilds the C++
    println!(
        "cargo:rerun-if-changed={}",
        sdk.join("bin/armv7-symbian-env").display()
    );
    sdk.join("bin")
}

/// A build for C++ (`cpp`) or C, `-O2`, C++20 for C++.
pub fn build(cpp: bool) -> cc::Build {
    let bin = sdk_bin();
    let mut b = cc::Build::new();
    b.no_default_flags(true)
        .cpp(cpp)
        .compiler(bin.join(if cpp {
            "armv7-symbian-g++"
        } else {
            "armv7-symbian-gcc"
        }))
        .archiver(bin.join("armv7-symbian-ar"))
        .flag("-O2");
    if cpp {
        b.flag("-std=gnu++20").flag("-fno-rtti");
    }
    b
}
