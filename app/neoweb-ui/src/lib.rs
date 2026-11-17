// SPDX-License-Identifier: GPL-3.0-only
//! NEOWEB's screens (Slint, no_std): the generated [`BrowserApp`] and its global [`BrowserState`]. NEOWEB.EXE
//! (symbian/neoweb) and the host snapshot tests drive the same UI; the logic is neoweb-core's.
//! Boards: the canvas Browser page, `br2-*` (docs browser-ui.md).
#![no_std]

extern crate alloc;

slint::include_modules!();
