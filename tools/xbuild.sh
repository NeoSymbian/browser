#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Cross-build for armv7-unknown-linux-gnueabihf in the docker/cross-armhf image (WB0).
#   tools/xbuild.sh image        build the image
#   tools/xbuild.sh servoshell   build stock servoshell v0.8.0 (step 2 smoke test) → out/servoshell
#   tools/xbuild.sh shell        a shell in the container
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
      cargo build --locked --release -p servoshell --target $TARGET \
        --no-default-features --features $FEATURES 2>&1 | tee /work/out/servoshell-build.log
      cp /target/$TARGET/release/servoshell /work/out/
      find /target/$TARGET/release/build -path '*mozjs_sys*' -name config.status -exec grep -l -- --disable-jit {} + \
        | head -1 | sed 's/^/jit off per: /' || echo 'no --disable-jit evidence found: check the mozjs build log'"
    file out/servoshell ;;
  shell) docker run --rm -it --platform linux/arm64 -v "$PWD":/work -v wb0-servo:/servo -v wb0-cargo:/cargo \
           -v wb0-target:/target -e CARGO_TARGET_DIR=/target "$IMAGE" bash ;;
  *) echo "usage: $0 image|servoshell|shell" >&2; exit 2 ;;
esac
