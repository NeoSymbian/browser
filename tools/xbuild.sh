#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Cross-build for armv7-unknown-linux-gnueabihf in the docker/cross-armhf image (WB0).
#   tools/xbuild.sh image        build the image
#   tools/xbuild.sh servoshell   build stock servoshell v0.8.0 (step 2 smoke test) → out/servoshell
#   tools/xbuild.sh shell        a shell in the container
# patches/<crate>/*.patch are applied to a copy of that locked crate (mozjs_sys today; moves to a fork later).
# JOBS (default 6) bounds parallelism: Docker Desktop's VM has 8 GB here.
# Big state lives in named volumes: wb0-servo (source), wb0-cargo (registry), wb0-target (build dir, ~40–60 GB).
set -euo pipefail
cd "$(dirname "$0")/.."

IMAGE=neosymbian-browser-cross:wb0
SERVO_TAG=v0.8.0
FEATURES=bundled,baked-in-resources,subtlecrypto
TARGET=armv7-unknown-linux-gnueabihf

run() {
  docker run --rm -i --platform linux/arm64 \
    -v "$PWD":/work -v wb0-servo:/servo -v wb0-cargo:/cargo -v wb0-target:/target \
    -e CARGO_TARGET_DIR=/target -e CARGO_BUILD_JOBS="${JOBS:-6}" "$IMAGE" bash -euo pipefail -c "$1"
}

case "${1:-}" in
  image) docker build --platform linux/arm64 -t "$IMAGE" docker/cross-armhf ;;
  servoshell)
    mkdir -p out
    run "
      [ -d /servo/.git ] || git clone --depth 1 --branch $SERVO_TAG https://github.com/servo/servo /servo
      cd /servo && git describe --tags --exact-match
      git checkout -q Cargo.lock   # the [patch] below rewrites it; fetch against Servo's own lock
      cargo fetch --locked >/dev/null
      # patches/<crate>/*.patch: copy the locked registry crate, apply, and patch it in (until the fork exists).
      cfg=()
      for d in /work/patches/*/; do
        c=\$(basename \$d); src=\$(ls -d /cargo/registry/src/*/\$c-[0-9]* | head -1)
        h=\$(cat \$d*.patch | sha256sum | cut -c1-16)
        if [ \"\$(cat /target/patched/\$c/.wb0-stamp 2>/dev/null)\" != \"\$h\" ]; then
          rm -rf /target/patched/\$c && mkdir -p /target/patched && cp -r \$src /target/patched/\$c
          for p in \$d*.patch; do patch -s -d /target/patched/\$c -p1 < \$p && echo applied \$p; done
          echo \$h > /target/patched/\$c/.wb0-stamp
        fi
        cfg+=(--config \"patch.crates-io.\$c.path='/target/patched/\$c'\")
      done
      cargo build --release -p servoshell --target $TARGET \"\${cfg[@]}\" \
        --no-default-features --features $FEATURES 2>&1 | tee /work/out/servoshell-build.log
      cp /target/$TARGET/release/servoshell /work/out/
      grep -h -o -- '--disable-jit' /target/$TARGET/release/build/mozjs_sys-*/out/build/reconfigure.inputs | head -1
      grep -h 'JS_CODEGEN_NONE' /target/$TARGET/release/build/mozjs_sys-*/out/build/js/src/js-confdefs.h | head -1"
    file out/servoshell ;;
  shell) docker run --rm -it --platform linux/arm64 -v "$PWD":/work -v wb0-servo:/servo -v wb0-cargo:/cargo \
           -v wb0-target:/target -e CARGO_TARGET_DIR=/target "$IMAGE" bash ;;
  *) echo "usage: $0 image|servoshell|shell" >&2; exit 2 ;;
esac
