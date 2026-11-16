#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Boot the WB0 armv7 guest. Screen on VNC 127.0.0.1:5901 (password symbian; console login root/symbian), serial on stdio, ssh on 127.0.0.1:2222:
#   ssh -i out/guest/guest_key -p 2222 root@127.0.0.1
# QEMU needs slirp ("user" netdev), which our symbian-qemu build lacks, so by default Debian's qemu-system-arm runs
# in a container. QEMU=/path/to/qemu-system-arm runs a host binary instead. MEM (MiB) and SMP size the guest.
set -euo pipefail
cd "$(dirname "$0")/../.."
G=out/guest
args=(-M virt,highmem=off -cpu cortex-a15 -smp "${SMP:-4}" -m "${MEM:-2048}" -accel tcg,thread=multi
  -kernel $G/vmlinuz -initrd $G/initrd.img
  -append "root=/dev/vda rw console=tty0 console=ttyAMA0 systemd.unified_cgroup_hierarchy=1"
  -drive if=none,file=$G/rootfs.img,format=raw,id=hd -device virtio-blk-device,drive=hd
  -netdev user,id=n,hostfwd=tcp:0.0.0.0:2222-:22 -device virtio-net-device,netdev=n
  -global virtio-mmio.force-legacy=false   # virtio-gpu needs modern virtio (else no console on VNC)
  -device virtio-rng-device -device virtio-gpu-device,xres=1080,yres=2160   # the phone's screen
  -device virtio-keyboard-device -device virtio-tablet-device -serial mon:stdio)   # VNC input for wb0/show.sh

if [ -n "${QEMU:-}" ]; then
  exec "$QEMU" "${args[@]}" -object secret,id=vncpw,data=symbian -vnc 127.0.0.1:1,password-secret=vncpw
fi
docker build -q -t neosymbian-browser-qemu:wb0 - >/dev/null <<'EOF'
FROM debian:trixie
RUN apt-get update && apt-get install -y --no-install-recommends qemu-system-arm && rm -rf /var/lib/apt/lists/*
EOF
exec docker run --rm -i --name wb0-guest -p 127.0.0.1:5901:5901 -p 127.0.0.1:2222:2222 -v "$PWD":/w -w /w \
  neosymbian-browser-qemu:wb0 qemu-system-arm "${args[@]}" -object secret,id=vncpw,data=symbian \
  -vnc 0.0.0.0:1,password-secret=vncpw
