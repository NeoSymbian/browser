#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# In the WB0 guest: show servoshell in a window on the guest screen (VNC 127.0.0.1:5901), for watching and poking at
# it by hand. This is the stock windowed path (cage = a one-window Wayland compositor, Mesa softpipe); the
# measurements stay headless.  show.sh [URL]   |   show.sh stop
set -euo pipefail
systemctl stop wb0-show 2>/dev/null || true
if [ "${1:-}" = stop ]; then systemctl start getty@tty1; exit 0; fi
systemctl stop getty@tty1   # cage needs tty1; `show.sh stop` gives the login prompt back
url=${1:-https://en.wikipedia.org/wiki/Symbian}
systemd-run --unit=wb0-show --collect -p TTYPath=/dev/tty1 -p StandardInput=tty-force -p StandardOutput=journal \
  -E LIBSEAT_BACKEND=seatd -E WLR_RENDERER=pixman -E XDG_RUNTIME_DIR=/run -E GALLIUM_DRIVER=softpipe -E LIBGL_ALWAYS_SOFTWARE=1 \
  -E DRAW_USE_LLVM=0 -E RUST_LOG=warn \
  cage -s -- /wb0/servoshell --pref js_disable_jit=true "$url"
echo "started; logs: journalctl -u wb0-show -f"
