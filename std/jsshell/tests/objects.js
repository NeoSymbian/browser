// SPDX-License-Identifier: Apache-2.0
// objects, classes, closures, destructuring, spread, getters, Proxy, Reflect
class Shape {
  #id;
  static count = 0;
  constructor(id) { this.#id = id; Shape.count++; }
  get id() { return this.#id; }
  area() { return 0; }
}
class Square extends Shape {
  constructor(id, side) { super(id); this.side = side; }
  area() { return this.side * this.side; }
}
const q = new Square(7, 3);
assertEq(q.id, 7);
assertEq(q.area(), 9);
assertEq(q instanceof Shape, true);
assertEq(Shape.count, 1);
function counter() { let n = 0; return () => ++n; }
const c = counter(); c(); c();
assertEq(c(), 3);
const { a, b: [x, ...rest], ...others } = { a: 1, b: [2, 3, 4], c: 5, d: 6 };
assertEq(a + x, 3);
assertDeepEq(rest, [3, 4]);
assertDeepEq(others, { c: 5, d: 6 });
const merged = { ...{ p: 1 }, ...{ p: 2, q: 3 } };
assertDeepEq(merged, { p: 2, q: 3 });
const o = { get twice() { return this.v * 2; }, v: 21 };
assertEq(o.twice, 42);
const log = [];
const p = new Proxy({}, { get(t, k) { log.push(k); return k.length; } });
assertEq(p.hello, 5);
assertDeepEq(log, ["hello"]);
assertEq(Reflect.has({ z: 1 }, "z"), true);
assertDeepEq(Object.entries({ u: 1, w: 2 }), [["u", 1], ["w", 2]]);
assertDeepEq(Object.fromEntries([["k", 1]]), { k: 1 });
const frozen = Object.freeze({ f: 1 });
assertThrows(() => { "use strict"; frozen.f = 2; }, TypeError);
assertEq(structuredCloneLike({ n: [1, { m: 2 }] }).n[1].m, 2);
function structuredCloneLike(v) { return JSON.parse(JSON.stringify(v)); }
assertEq(typeof globalThis, "object");
assertEq(new Function("a", "b", "return a * b")(6, 7), 42);
assertEq(eval("1 + 2"), 3);
const wm = new WeakMap(), key = {};
wm.set(key, "v");
assertEq(wm.get(key), "v");
