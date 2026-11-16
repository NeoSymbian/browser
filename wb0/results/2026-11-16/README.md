# WB0 smoke test, 2026-11-16

Servo 0.8.0 (`4e136d8ef`), `armv7-unknown-linux-gnueabihf`, mozjs built `--disable-jit` (`JS_CODEGEN_NONE`), plus
`--pref js_disable_jit=true`. Guest: QEMU `virt` cortex-a15 ×4, 2 GiB, Debian 13 armhf, Mesa softpipe.

- `wiki-window-softpipe.png`: https://en.wikipedia.org/wiki/Symbian in windowed servoshell (cage, 1080×2160,
  DPR 1), captured with `grim`. Text, images, infobox and links render.
  - The icons drawn with CSS `mask-image` + SVG show as black squares (not supported yet, unconfirmed).
  - DPR 1 gives the desktop layout; the phone needs DPR ≈ 2.5–3.
- **Headless (`--headless -o --exit`), same page:**
  - load completes in about 15 s (emulated, not gating);
  - 38 threads, peak VmPeak 832 MiB;
  - RSS grows about 1 MiB/s after the load (300 → 468 MiB in 3 minutes), and `--exit` never sees a stable image.
    Under investigation.
- **Web compatibility seen so far:** `IntersectionObserver` is behind `dom_intersection_observer_enabled`, off by
  default, so Wikipedia's scripts throw a `ReferenceError`.
