// SPDX-License-Identifier: Apache-2.0
// GC stress: churn, big and small objects, weak references, collections in between; the shell checks that a
// shrinking GC gives the chunks back (gcChunks before / after)
var chunksBefore = gcChunks();
var keep = [];
for (let round = 0; round < 40; round++) {
  let garbage = [];
  for (let i = 0; i < 20000; i++) garbage.push({ i, s: "str" + i, a: [i, i + 1] });
  keep.push(garbage[round]);
  if (round % 4 === 0) {
    const bigs = [];
    for (let j = 0; j < 8; j++) bigs.push(new Float64Array(1 << 16));  // 512 KiB each, outside the GC heap
    bigs[7][100] = round;
    assertEq(bigs[7][100], round);
  }
  if (round % 10 === 9) gc();
}
assertEq(keep.length, 40);
assertEq(keep[39].i, 39);
var weak = new WeakRef({ big: new Array(1000).fill(7) });
var registry = new FinalizationRegistry(() => {});
registry.register({}, "held");
var strings = new Set();
for (let i = 0; i < 50000; i++) strings.add("k" + (i % 5000));
assertEq(strings.size, 5000);
var chunksPeak = gcChunks();
keep = null; strings = null;
