# WB0 quick verdict (2026-11-16)

Each of the 10 sites loaded once with Servo 0.8.0 on 32-bit ARM: JIT off, SWGL (the phone's software GL), the phone's
screen (1080×2160, DPR 2.75, Android UA), a 1 GiB cap, in the QEMU guest. Each run is load, screenshot and memory
report.

| Site | Result | Peak memory | Threads |
|---|---|---:|---:|
| Wikipedia | ✅ | 210 MiB | 40 |
| GitHub | ✅ | 169 MiB | 110 |
| Stack Overflow | ✅ | 45 MiB | 38 |
| TodoMVC | ✅ (full workflow passed earlier: typing, taps) | 50 MiB | 38 |
| YouTube | ✅ (no media until WB7) | 37 MiB | 38 |
| DuckDuckGo | ❌ web-font sanitiser crash | 68 MiB | 36 |
| BBC | ❌ same | 120 MiB | 129 |
| WordPress | ❌ same | 80 MiB | 39 |
| Gmail | ❌ same | 109 MiB | 36 |
| Google Docs | ❌ same | 92 MiB | 120 |

**Verdict: GO.**
- **Memory fits easily:** 37–210 MiB per page, Servo's own share ~45 MiB, against the phone's ~2 GB.
- **SWGL works:** pixel-identical to Mesa, at a third of the memory.
- **All 5 failures are one bug:** OTS (the `fontsan` font sanitiser) asserts `(Tell() & 3) == 0` on web fonts on
  32-bit. It must be fixed before WB4.
- **Not measured here:**
  - speed, which needs real hardware (WB0p);
  - full per-site workflows (only TodoMVC's ran);
  - soaks.
- **Design rule found:** the embedder must pace scroll input to painted frames, or WebRender's work piles up.
