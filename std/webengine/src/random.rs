// SPDX-License-Identifier: Apache-2.0
//! getrandom on NeoSymbian: every version Servo pulls in (0.2, 0.3, 0.4) fills from the kernel's secure RNG
//! (`Exec::MathSecureRandom`, WB1). 0.3 and 0.4 call `__getrandom_v03_custom` (built with
//! `--cfg getrandom_backend="custom"`, .cargo/config.toml); 0.2 takes a registered function.

/// The kernel's RNG, in pieces (a descriptor holds at most 256 MiB, far more than any caller asks for).
fn fill(dest: &mut [u8]) -> Result<(), i32> {
    for chunk in dest.chunks_mut(4096) {
        match e32::sys::math_secure_random(chunk) {
            0 => {}
            err => return Err(err),
        }
    }
    Ok(())
}

/// getrandom 0.3 and 0.4's custom backend.
///
/// # Safety
/// getrandom passes a writable buffer of `len` bytes.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(dest: *mut u8, len: usize) -> Result<(), getrandom03::Error> {
    // SAFETY: as documented above.
    let dest = unsafe { core::slice::from_raw_parts_mut(dest, len) };
    // getrandom's "unexpected" error: the RNG not seeded yet (KErrNotReady), or any other kernel error
    fill(dest).map_err(|_| getrandom03::Error::UNEXPECTED)
}

fn getrandom02(dest: &mut [u8]) -> Result<(), getrandom02::Error> {
    fill(dest).map_err(|e| {
        // a custom error code: getrandom 0.2 keeps it for Display
        getrandom02::Error::from(core::num::NonZeroU32::new(getrandom02::Error::CUSTOM_START + e.unsigned_abs()).unwrap())
    })
}
getrandom02::register_custom_getrandom!(getrandom02);

