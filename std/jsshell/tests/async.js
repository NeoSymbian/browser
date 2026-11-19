// SPDX-License-Identifier: Apache-2.0
// promises, async/await, async generators: lib.rs drains the job queue after the script and checks `asyncDone`
var asyncDone = false;
var order = [];
async function step(v) { await null; order.push(v); return v * 2; }
async function* agen() { yield 1; await Promise.resolve(); yield 2; }
(async () => {
  const r = await Promise.all([step(1), step(2)]);
  assertDeepEq(r, [2, 4]);
  const got = [];
  for await (const v of agen()) got.push(v);
  assertDeepEq(got, [1, 2]);
  const settled = await Promise.allSettled([Promise.reject(new Error("no")), 3]);
  assertEq(settled[0].status, "rejected");
  assertEq(await Promise.any([Promise.reject(1), Promise.resolve(9)]), 9);
  let caught = false;
  try { await Promise.reject(new TypeError("t")); } catch (e) { caught = e instanceof TypeError; }
  assertEq(caught, true);
  const { promise, resolve } = Promise.withResolvers();
  queueMicrotaskLike(() => resolve("later"));
  assertEq(await promise, "later");
  assertDeepEq(order, [1, 2]);
  asyncDone = true;
})().catch((e) => { asyncError = String(e); });
var asyncError = null;
function queueMicrotaskLike(f) { Promise.resolve().then(f); }
