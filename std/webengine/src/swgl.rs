// SPDX-License-Identifier: Apache-2.0
//! `--gl swgl`: Servo's `RenderingContext` over SWGL, WebRender's software GL (Firefox's). WebRender recognises it
//! by its renderer string ("Software WebRender"). Headless Servo 0.8 never calls `glow_gl_api` (only servoshell's
//! GUI does), so the glow context is a stub that answers glow's start-up queries and nothing else.
//! The seed of WB4's NeoUiRenderingContext.

use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::rc::Rc;
use std::sync::Arc;

use dpi::PhysicalSize;
use gleam::gl::{self, Gl};
use servo::{DeviceIntRect, RenderingContext, RgbaImage};

pub struct SwglRenderingContext {
    context: swgl::Context,
    gl: Rc<dyn Gl>,
    glow: Arc<glow::Context>,
    size: Cell<PhysicalSize<u32>>,
}

impl SwglRenderingContext {
    pub fn new(size: PhysicalSize<u32>) -> Self {
        let context = swgl::Context::create();
        context.make_current();
        let this = Self {
            context,
            gl: Rc::new(context),
            // SAFETY: the loader only hands out the stubs below, which match glow's C signatures.
            glow: Arc::new(unsafe { glow::Context::from_loader_function(glow_stub) }),
            size: Cell::new(size),
        };
        this.resize(size);
        this
    }
}

impl SwglRenderingContext {
    /// The painted frame into `dst` as RGBA8 rows top-down, `dst_stride` bytes apart, at most `max_w` × `max_h`:
    /// one pass from SWGL's colour buffer (BGRA, rows bottom-up), instead of read_to_image's read, flip and copy.
    /// Returns the size written.
    pub fn copy_frame(
        &self,
        dst: *mut u8,
        dst_stride: usize,
        max_w: u32,
        max_h: u32,
    ) -> Option<(u32, u32)> {
        let (buf, w, h, stride) = self.context.get_color_buffer(0, true);
        if buf.is_null() || w <= 0 || h <= 0 {
            return None;
        }
        let (w, h) = ((w as u32).min(max_w), (h as u32).min(max_h));
        for row in 0..h as usize {
            // SAFETY: SWGL's buffer holds h rows of `stride` bytes; dst holds max_h rows of dst_stride >= 4 * w
            let (src, out) = unsafe {
                (
                    std::slice::from_raw_parts(
                        (buf as *const u8).add((h as usize - 1 - row) * stride as usize)
                            as *const u32,
                        w as usize,
                    ),
                    std::slice::from_raw_parts_mut(
                        dst.add(row * dst_stride) as *mut u32,
                        w as usize,
                    ),
                )
            };
            for (o, &v) in out.iter_mut().zip(src) {
                // BGRA in memory (little-endian 0xAARRGGBB) to RGBA (0xAABBGGRR), opaque
                *o = 0xff00_0000 | (v & 0x0000_ff00) | ((v >> 16) & 0xff) | ((v & 0xff) << 16);
            }
        }
        Some((w, h))
    }
}

impl RenderingContext for SwglRenderingContext {
    fn read_to_image(&self, rect: DeviceIntRect) -> Option<RgbaImage> {
        // As Servo's own contexts do: read the default framebuffer, then flip it (GL rows go bottom-up).
        self.gl.bind_framebuffer(gl::FRAMEBUFFER, 0);
        let (w, h) = (rect.width() as usize, rect.height() as usize);
        let pixels = self.gl.read_pixels(
            rect.min.x,
            rect.min.y,
            w as i32,
            h as i32,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
        );
        if pixels.len() < w * h * 4 {
            return None;
        }
        let stride = w * 4;
        let flipped: Vec<u8> = pixels[..h * stride]
            .chunks_exact(stride)
            .rev()
            .flatten()
            .copied()
            .collect();
        RgbaImage::from_raw(w as u32, h as u32, flipped)
    }

    fn size(&self) -> PhysicalSize<u32> {
        self.size.get()
    }

    fn resize(&self, size: PhysicalSize<u32>) {
        self.size.set(size);
        // A null buffer lets SWGL allocate the colour buffer itself (stride 0 = tightly packed).
        self.context.init_default_framebuffer(
            0,
            0,
            size.width as i32,
            size.height as i32,
            0,
            std::ptr::null_mut(),
        );
    }

    fn present(&self) {}

    fn make_current(&self) -> Result<(), surfman::Error> {
        self.context.make_current();
        Ok(())
    }

    fn gleam_gl_api(&self) -> Rc<dyn Gl> {
        self.gl.clone()
    }

    fn glow_gl_api(&self) -> Arc<glow::Context> {
        self.glow.clone()
    }
}

impl Drop for SwglRenderingContext {
    fn drop(&mut self) {
        self.context.destroy();
    }
}

/// glow's loader: only the queries `glow::Context` makes while it is created. Any other GL call through this context
/// would hit a null function pointer; headless Servo 0.8 makes none.
fn glow_stub(name: &str) -> *const c_void {
    match name {
        "glGetString" => get_string as *const c_void,
        "glGetStringi" => get_string_i as *const c_void,
        "glGetIntegerv" => get_integer_v as *const c_void,
        _ => std::ptr::null(),
    }
}

const GL_VERSION: u32 = 0x1F02;

extern "C" fn get_string(name: u32) -> *const c_char {
    let s: &'static [u8] = if name == GL_VERSION {
        b"OpenGL ES 3.0 SWGL stub\0"
    } else {
        b"\0"
    };
    s.as_ptr().cast()
}

extern "C" fn get_string_i(_name: u32, _index: u32) -> *const c_char {
    c"".as_ptr()
}

extern "C" fn get_integer_v(_name: u32, data: *mut i32) {
    if !data.is_null() {
        // SAFETY: glow passes a pointer to one i32 (NUM_EXTENSIONS).
        unsafe { *data = 0 };
    }
}
