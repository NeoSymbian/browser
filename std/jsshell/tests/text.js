// SPDX-License-Identifier: Apache-2.0
// JSON, RegExp, Date (UTC), errors
const doc = { name: "neo", list: [1, 2.5, true, null, "s"], nested: { deep: { x: "é\n" } } };
assertDeepEq(JSON.parse(JSON.stringify(doc)), doc);
assertEq(JSON.stringify({ a: [1, { b: 2 }] }, null, 1).split("\n").length, 8);
assertThrows(() => JSON.parse("{bad"), SyntaxError);
const re = /(\d{4})-(\d{2})-(\d{2})/;
const [, y, mo, d] = re.exec("on 2026-11-19.");
assertEq(y + mo + d, "20261119");
assertEq("a1b22c333".replace(/\d+/g, (n) => n.length), "a1b2c3");
const named = /(?<word>[A-Za-zÀ-ÿ]+)/u.exec("…Ünïcode!");  // no \p{...}: that needs ICU (no Intl here)
assertEq(named.groups.word, "Ünïcode");
assertDeepEq([..."x1y2".matchAll(/\d/g)].map((m) => m[0]), ["1", "2"]);
assertEq(/^[\w.]+@[\w.]+$/.test("a.b@c.d"), true);
assertEq(/a.c/s.test("a\nc"), true);
const dt = new Date(Date.UTC(2026, 10, 19, 12, 30, 45, 500));
assertEq(dt.toISOString(), "2026-11-19T12:30:45.500Z");
assertEq(dt.getUTCDay(), 4);
assertEq(Date.parse("2000-01-01T00:00:00Z"), 946684800000);
assertEq(new Date(0).getTime(), 0);
assertEq(typeof Date.now(), "number");
try { null.x; } catch (e) { assertEq(e instanceof TypeError, true); }
const err = new RangeError("r", { cause: 1 });
assertEq(err.cause, 1);
assertEq(String(new Error("m")), "Error: m");
function deep(n) { return n ? deep(n - 1) + 1 : 0; }
assertEq(deep(5000), 5000);
assertThrows(() => { (function f() { f(); })(); }, InternalError, "too much recursion is an InternalError");
