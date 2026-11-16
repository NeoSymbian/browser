#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Build the WB0 armhf guest: out/guest/{vmlinuz,initrd.img,rootfs.img,guest_key}.
# Needs Docker with linux/arm/v7 emulation (Docker Desktop on the Mac has it).
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT=out/guest SIZE=${SIZE:-8G}
mkdir -p "$OUT"

[ -f "$OUT/guest_key" ] || ssh-keygen -q -t ed25519 -N '' -C wb0-guest -f "$OUT/guest_key"
cp "$OUT/guest_key.pub" wb0/guest/guest_key.pub
docker build --platform linux/arm/v7 -t neosymbian-browser-guest:wb0 wb0/guest
rm wb0/guest/guest_key.pub

cid=$(docker create --platform linux/arm/v7 neosymbian-browser-guest:wb0)
trap 'docker rm -f "$cid" >/dev/null' EXIT
docker cp -L "$cid:/vmlinuz" "$OUT/vmlinuz"
docker cp -L "$cid:/initrd.img" "$OUT/initrd.img"
docker export "$cid" > "$OUT/rootfs.tar"

# ext4 from the tarball, as root in a helper container so ownership and modes survive.
docker run --rm --platform linux/arm64 -v "$PWD/$OUT":/out debian:trixie bash -euo pipefail -c "
  apt-get update -qq && apt-get install -y -qq e2fsprogs >/dev/null
  mkdir /r && tar -xpf /out/rootfs.tar -C /r && rm /out/rootfs.tar
  rm -f /out/rootfs.img && truncate -s $SIZE /out/rootfs.img && mkfs.ext4 -q -d /r -L wb0 /out/rootfs.img"
ls -lh "$OUT"
