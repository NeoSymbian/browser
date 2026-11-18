#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# The `neosymbian` Rust toolchain (docs browser-std.md, WB2c): the pinned nightly with NeoSymbian/rust's `library/`
# (target_os = "symbian") as its rust-src, so `-Zbuild-std` builds std for armv7a-symbian-eabi. The shared nightly
# stays untouched: this is a copy, registered with `rustup toolchain link`.
#
#   tools/neosymbian-toolchain.sh RUST_CHECKOUT [LIBC_CHECKOUT]
#
# RUST_CHECKOUT: NeoSymbian/rust on branch neosymbian (only library/ is read). LIBC_CHECKOUT: a local
# NeoSymbian/libc to use instead of its git branch. Run again after changing either; it is quick.
set -eu
NIGHTLY=nightly-2026-10-20	# = e32-rs, neoui and browser/symbian; NeoSymbian/rust's neosymbian branch sits on its commit (07e526db9)
RUST=$(cd "${1:?usage: $0 RUST_CHECKOUT [LIBC_CHECKOUT]}" && pwd)
LIBC=${2:+$(cd "$2" && pwd)}
DEST=${NEOSYMBIAN_TOOLCHAIN:-$HOME/.local/share/neosymbian-toolchain}

rustup toolchain install "$NIGHTLY" --profile minimal --component rust-src >/dev/null
SRC=$(rustup run "$NIGHTLY" rustc --print sysroot)
if [ ! -x "$DEST/bin/rustc" ]; then
	mkdir -p "$DEST"
	cp -R "$SRC/." "$DEST/"
fi
LIB="$DEST/lib/rustlib/src/rust/library"
# the fork's library/ over the nightly's (keeping the nightly's vendored crates and their .cargo config)
rsync -a --delete --exclude /vendor --exclude /.cargo --exclude /Cargo.lock "$RUST/library/" "$LIB/"
[ -d "$RUST/library/backtrace/src" ] || cp -R "$SRC/lib/rustlib/src/rust/library/backtrace/." "$LIB/backtrace/"
if [ -n "$LIBC" ]; then
	sed -i.bak "s|^libc = { git = .*|libc = { path = '$LIBC' }|" "$LIB/Cargo.toml" && rm -f "$LIB/Cargo.toml.bak"
fi
rustup toolchain link neosymbian "$DEST"
echo "toolchain neosymbian: $DEST (library from $RUST${LIBC:+, libc from $LIBC})"
echo "after a library/ change, cargo clean the workspaces using it: cargo does not see changes in build-std sources"
