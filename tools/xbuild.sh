#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Cross-build for armv7-unknown-linux-gnueabihf in the docker/cross-armhf image (WB0).
#   tools/xbuild.sh image        build the image
#   tools/xbuild.sh servoshell   stock servoshell v0.8.0 → out/servoshell
#   tools/xbuild.sh neoweb       our embedder (crates/neoweb-host) → out/neoweb-host; FEATURES=js_jit for the JIT build
#   tools/xbuild.sh check        fmt + armv7 clippy -D warnings for crates/
#   tools/xbuild.sh stackprobe   out/stackprobe.so for the guest
#   tools/xbuild.sh shell        a shell in the container
# patches/<crate>/*.patch are applied to a copy of that locked crate (mozjs_sys today; also on its fork, see patches/).
# JOBS (default 6) bounds parallelism: Docker Desktop's VM memory limits it.
# Big state lives in named volumes: wb0-servo (source), wb0-cargo (registry), wb0-target (build dir, ~40–60 GB).
set -euo pipefail
cd "$(dirname "$0")/.."

IMAGE=neosymbian-browser-cross:wb0
SERVO_TAG=v0.8.0
SERVOSHELL_FEATURES=bundled,baked-in-resources,subtlecrypto
TARGET=armv7-unknown-linux-gnueabihf

run() {
  docker run --rm -i --platform linux/arm64 \
    -v "$PWD":/work -v wb0-servo:/servo -v wb0-cargo:/cargo -v wb0-target:/target \
    -e CARGO_TARGET_DIR="${TARGET_DIR:-/target}" -e CARGO_BUILD_JOBS="${JOBS:-6}" "$IMAGE" \
    bash -euo pipefail -c "$PRELUDE
$1"
}

# Shared by every build: the Servo checkout, its crates fetched, and the patched crate copies in /target/patched.
PRELUDE="
[ -d /servo/.git ] || git clone --depth 1 --branch $SERVO_TAG https://github.com/servo/servo /servo
cd /servo && git describe --tags --exact-match >/dev/null && git checkout -q Cargo.lock
cargo fetch --locked >/dev/null
patch_cfg=()
for d in /work/patches/*/; do
  c=\$(basename \$d); src=\$(ls -d /cargo/registry/src/*/\$c-[0-9]* | head -1)
  h=\$(cat \$d*.patch | sha256sum | cut -c1-16)
  if [ \"\$(cat /target/patched/\$c/.wb0-stamp 2>/dev/null)\" != \"\$h\" ]; then
    rm -rf /target/patched/\$c && mkdir -p /target/patched && cp -r \$src /target/patched/\$c
    for p in \$d*.patch; do patch -s -d /target/patched/\$c -p1 < \$p && echo applied \$p; done
    echo \$h > /target/patched/\$c/.wb0-stamp
  fi
  patch_cfg+=(--config \"patch.crates-io.\$c.path='/target/patched/\$c'\")
done
"

jit_evidence='
grep -h -o -- "--disable-jit" $CARGO_TARGET_DIR/'$TARGET'/release/build/mozjs_sys-*/out/build/reconfigure.inputs | head -1 || echo "JIT enabled"
'

case "${1:-}" in
  image) docker build --platform linux/arm64 -t "$IMAGE" docker/cross-armhf ;;
  servoshell)
    mkdir -p out
    run "
      cargo build --release -p servoshell --target $TARGET \"\${patch_cfg[@]}\" \
        --no-default-features --features $SERVOSHELL_FEATURES 2>&1 | tee /work/out/servoshell-build.log
      cp \$CARGO_TARGET_DIR/$TARGET/release/servoshell /work/out/
      $jit_evidence"
    file out/servoshell ;;
  neoweb)
    mkdir -p out
    run "
      cd /work/crates
      [ -f Cargo.lock ] || cp /servo/Cargo.lock Cargo.lock   # seed: the exact versions servoshell was built with
      cargo build --release -p neoweb-host --target $TARGET ${FEATURES:+--features $FEATURES} 2>&1 \
        | tee /work/out/neoweb-build.log
      cp \$CARGO_TARGET_DIR/$TARGET/release/neoweb-host /work/out/neoweb-host${FEATURES:+-$FEATURES}
      $jit_evidence"
    file out/neoweb-host* ;;
  check)
    run "
      cd /work/crates
      rustup component add rustfmt clippy >/dev/null 2>&1
      cargo fmt --check
      cargo clippy --release -p neoweb-host --target $TARGET -- -D warnings" ;;
  stackprobe)
    mkdir -p out
    run "arm-linux-gnueabihf-gcc -O2 -Wall -Werror -shared -fPIC -o /work/out/stackprobe.so /work/tools/stackprobe.c -ldl -lpthread"
    file out/stackprobe.so ;;
  shell) docker run --rm -it --platform linux/arm64 -v "$PWD":/work -v wb0-servo:/servo -v wb0-cargo:/cargo \
           -v wb0-target:/target -e CARGO_TARGET_DIR=/target "$IMAGE" bash ;;
  *) echo "usage: $0 image|servoshell|neoweb|check|stackprobe|shell" >&2; exit 2 ;;
esac
