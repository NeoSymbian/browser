// SPDX-License-Identifier: Apache-2.0
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/cxxrt.cpp");
    symbian_cc::build(true)
        .file("src/cxxrt.cpp")
        .compile("cxxrt");

    // the self-contained pieces of the SDK's libstdc++ (built for newlib, never linked whole): std::map/set's
    // red-black tree, std::list's hook, std::unordered_map's rehash policy (only floor() from libm)
    let bin = symbian_cc::sdk_bin();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let lib = Command::new(bin.join("armv7-symbian-gcc"))
        .arg("-print-file-name=libstdc++.a")
        .output()
        .unwrap();
    let lib = String::from_utf8(lib.stdout).unwrap().trim().to_string();
    let members = ["tree.o", "list.o", "hashtable_c++0x.o"];
    let ok = Command::new(bin.join("armv7-symbian-ar"))
        .arg("x")
        .arg(&lib)
        .args(members)
        .current_dir(&out)
        .status();
    assert!(
        ok.map(|s| s.success()).unwrap_or(false),
        "extracting {members:?} from {lib}"
    );
    let archive = out.join("libcxxrt_stl.a");
    let _ = std::fs::remove_file(&archive);
    let ok = Command::new(bin.join("armv7-symbian-ar"))
        .arg("rcs")
        .arg(&archive)
        .args(members)
        .current_dir(&out)
        .status();
    assert!(
        ok.map(|s| s.success()).unwrap_or(false),
        "archiving libcxxrt_stl.a"
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=cxxrt_stl");
}
