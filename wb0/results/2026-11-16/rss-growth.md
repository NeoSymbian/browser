# WB0: the RSS growth after load (diagnosis, 2026-11-16)

**Setup:**
- Servo 0.8.0 armv7, JIT off, Mesa softpipe;
- the QEMU guest (cortex-a15 ×4, 2 GiB);
- https://en.wikipedia.org/wiki/Symbian;
- a 1 GiB cgroup cap.

**Raw runs:**
- `idle-every`, `idle-5s`, `idle-shots`, `scroll` (neoweb-host `wb0/diag/*.json`);
- each has Servo memory reports (`*.mem.json`), `smaps.py` snapshots every 30 s and `measure.sh` CSVs.

## What was seen
- **Stock servoshell, headless, `--exit`:**
  - the page never exits;
  - RSS grows ~1 MiB/s after load (300 → 482 MiB in 5 min);
  - CPU is mostly idle.
- **Controls:**
  - a static local page exits in 7 s at 108 MiB;
  - the same Wikipedia HTML with the scripts stripped exits in 70 s at 198 MiB, with flat RSS;
  - IntersectionObserver on: the JS errors go, the growth stays.
- **First neoweb-host smoke run** (DPR 2.75, a 1500 px wheel scroll, before the scroll-step fix):
  - the anonymous heap went to the 1 GiB cap, with VmPeak 2 GB;
  - 1 frame was painted in 134 s, and the process stalled in reclaim.

## Findings
1. **Idle doesn't run away.** Idle Wikipedia at DPR 2.75 (`idle-*`) peaks at 287–345 MiB whatever the paint
   frequency (every frame, every 5 s, screenshots only), and exits cleanly.
2. **Servo's own memory is small and flat.** The memory report gives about 45 MiB explicit:
   - JS heap 18–20;
   - layout 14.6;
   - WebRender images + display list 6.1;
   - caches ~6.

   It doesn't change while scrolling (`scroll`, s0 → s5).
3. **The rest is software-GL memory that gets touched lazily.**
   - While scrolling in 300 px steps, resident memory grew 302 → 442 MiB, all in anonymous mappings
     (160 → 278 MiB [anon]);
   - meanwhile the heap allocation (390 MiB), the mapping count (237) and VmSize (798 MiB) stayed flat.

   Memory already reserved is filled as more of the page is rasterised: WebRender's texture and picture caches and
   render targets, kept by Mesa softpipe in ordinary memory. It is bounded by the reservation, not a leak.
4. **Mesa costs code memory:** libLLVM ~34 MiB resident even with softpipe, gallium ~9 MiB. SWGL (WB4) avoids LLVM.
5. **The guest kernel's PSS is unreliable** (lower than Private_Dirty). WB0 reports RSS, RssAnon and the cgroup
   figures, not PSS.
6. **Speed of a frame:** one 1080×2160 frame takes ~10 s with softpipe under TCG, so input steps take ~10 s each.
   It is emulation, not gating (the speed gate WB0p needs real hardware).

## Open
- **The 1 GiB runaway** (big scroll while the page animates, VmPeak 2 GB): being re-checked with the fixed harness.
  If it repeats, the next steps are WebRender's picture-cache and texture-cache budgets, and the same run with
  `--paint-interval`.
- **The `--exit` hang in servoshell:** its stable-image readiness never settles on Wikipedia (the page keeps toggling
  `animating`). That is servoshell's screenshot logic, not memory; neoweb-host doesn't use it.

## Decision (plan rule)
- Not a structural leak: GL/texture memory, bounded by its reservation.
- The memory budget for the phone is set by the software GL's texture and picture caches. WB0 records it per site,
  and the WebRender cache limits are the tunable to try at 512 MiB.
