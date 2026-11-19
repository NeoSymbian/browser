// SPDX-License-Identifier: Apache-2.0
// JSSHELL's test helpers (lib.rs runs this in every global): assertions over the native print/gc.
"use strict";
function describe(v) {
  try { return typeof v === "string" ? JSON.stringify(v) : String(v); } catch (e) { return "(" + typeof v + ")"; }
}
function assertEq(actual, expected, what) {
  if (!Object.is(actual, expected)) {
    throw new Error((what ? what + ": " : "") + "got " + describe(actual) + ", expected " + describe(expected));
  }
}
function assertDeepEq(actual, expected, what) {
  assertEq(JSON.stringify(actual), JSON.stringify(expected), what);
}
function assertThrows(f, ctor, what) {
  try { f(); } catch (e) {
    if (ctor && !(e instanceof ctor)) throw new Error((what || "assertThrows") + ": threw " + describe(e));
    return e;
  }
  throw new Error((what || "assertThrows") + ": nothing thrown");
}
