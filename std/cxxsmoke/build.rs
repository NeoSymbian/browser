// SPDX-License-Identifier: Apache-2.0
fn main() {
    println!("cargo:rerun-if-changed=src/smoke.cpp");
    symbian_cc::build(true)
        .file("src/smoke.cpp")
        .compile("smoke");
}
