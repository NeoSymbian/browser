# NeoSymbian browser

A web browser for NeoSymbian, built on [Servo](https://servo.org). Design, decisions and stages (WB0–WB7):
[docs/browser.md](https://github.com/NeoSymbian/docs/blob/main/apps/browser/README.md).

## WB0: host proof (emulated)

Servo v0.8.0 cross-built for `armv7-unknown-linux-gnueabihf` and measured in a QEMU armv7 guest: function on 10
sites, memory under 512 MiB / 1 GiB caps, virtual address space against 2 GiB, threads and stack peaks. The speed gate
(WB0p) needs a physical ARM board and is deferred.

| Path | What |
|---|---|
| `docker/cross-armhf/` | The cross-build image (Debian trixie, arm64 host on the Mac) |
| `tools/xbuild.sh` | `image`, `servoshell` (stock servoshell → `out/servoshell`), `shell` |
| `tools/stackprobe.c` | `LD_PRELOAD` per-thread stack high-water probe; `tools/stackprobe_check.sh` is its self-test |
| `wb0/guest/` | The Debian armhf guest: `mkimage.sh` → `out/guest/`, `run.sh` boots it (VNC 127.0.0.1:5901, ssh :2222) |
| `wb0/measure.sh` | In the guest: a command under a cgroup memory cap, sampled once a second to CSV |

```sh
tools/xbuild.sh image && tools/xbuild.sh servoshell     # Docker Desktop, ≥ 120 GB disk
wb0/guest/mkimage.sh && wb0/guest/run.sh                 # QEMU in a container; QEMU= for a host binary
scp -i out/guest/guest_key -P 2222 out/servoshell wb0/measure.sh root@127.0.0.1:/wb0/
# in the guest:
GALLIUM_DRIVER=softpipe LIBGL_ALWAYS_SOFTWARE=1 DRAW_USE_LLVM=0 /wb0/measure.sh 1G /wb0/wiki.csv -- \
  /wb0/servoshell --headless --window-size 1080x2160 --screen-size 1080x2160 --device-pixel-ratio 1 \
  --pref js_disable_jit=true -o /wb0/wiki.png --exit https://en.wikipedia.org/
```

## Licence

Apache-2.0. Servo is MPL-2.0.
