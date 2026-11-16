# Crate patches

`tools/xbuild.sh` copies each locked crate named here from the Cargo registry, applies `<crate>/*.patch`, and
patches it in with `--config patch.crates-io.<crate>.path=…`. A patch lives here only until upstream takes it; the
same change sits on the crate's NeoSymbian fork, branch `neosymbian`.

| Crate | Patch | Fork commit | Upstream |
|---|---|---|---|
| mozjs_sys 153.6.0-1 | `0001-arm32-nojit-atomics.patch`: the 32-bit ARM `--disable-jit` build | [NeoSymbian/mozjs@3049613](https://github.com/NeoSymbian/mozjs/commit/3049613767c296b2e9252dd1341db8958f1d9a16) (`mozjs-sys/etc/patches/0055`) | candidate, not sent |
