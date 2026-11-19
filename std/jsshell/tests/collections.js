// SPDX-License-Identifier: Apache-2.0
// arrays, Map/Set, typed arrays, sorting, iteration
const arr = Array.from({ length: 1000 }, (_, i) => (i * 7919) % 1000);
arr.sort((p, q) => p - q);
for (let i = 1; i < arr.length; i++) if (arr[i - 1] > arr[i]) throw new Error("sort order");
assertEq(arr[0], 0);
assertEq(arr.at(-1), 999);
assertDeepEq([3, 1, 2].toSorted(), [1, 2, 3]);
assertDeepEq([1, [2, [3, [4]]]].flat(Infinity), [1, 2, 3, 4]);
assertEq([1, 2, 3, 4].reduce((s, v) => s + v, 0), 10);
assertEq([5, 6, 7].findLast((v) => v < 7), 6);
assertDeepEq(Object.groupBy([1, 2, 3, 4], (v) => (v % 2 ? "odd" : "even")), { odd: [1, 3], even: [2, 4] });
const m = new Map([[1, "a"], ["k", { v: 2 }]]);
m.set(NaN, "nan");
assertEq(m.get(NaN), "nan");
assertEq(m.size, 3);
const st = new Set("mississippi");
assertEq([...st].join(""), "misp");
const ta = new Float64Array([1.5, 2.5, 3.5]);
assertEq(ta.reduce((s, v) => s + v), 7.5);
const u8 = new Uint8Array(new ArrayBuffer(8));
u8.set([1, 2, 3, 255]);
const dv = new DataView(u8.buffer);
assertEq(dv.getUint32(0, true), 0xff030201);
assertEq(new Uint8ClampedArray([300, -5])[0], 255);
const big = new BigInt64Array(2);
big[0] = -1n;
assertEq(big[0], -1n);
function* gen(n) { for (let i = 0; i < n; i++) yield i * i; }
assertDeepEq([...gen(5)], [0, 1, 4, 9, 16]);
const it = { from: 1, to: 3, [Symbol.iterator]() { let c = this.from, t = this.to; return { next: () => (c <= t ? { value: c++, done: false } : { value: undefined, done: true }) }; } };
assertDeepEq(Array.from(it), [1, 2, 3]);
assertDeepEq(Array.from(new Map([[1, 2]]).entries()), [[1, 2]]);
