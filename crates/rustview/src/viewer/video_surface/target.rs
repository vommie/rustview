//! A texture with a framebuffer object around it: mpv renders into it, Slint shows it as an image.

use glow::HasContext as _;
use rustview_core::util::cast::SaturatingAs;

/// An RGBA8 texture of `width` x `height` pixels and its framebuffer.
pub struct VideoTarget {
    pub texture: glow::NativeTexture,
    pub framebuffer: glow::NativeFramebuffer,
    pub width: u32,
    pub height: u32,
}

/// A GL enum as the `GLint` that some entry points take for it (every enum used here fits).
fn param(value: u32) -> i32 {
    value.saturating_as::<i32>()
}

impl VideoTarget {
    /// Allocates the texture and its framebuffer on the current context and leaves both unbound.
    pub fn new(gl: &glow::Context, width: u32, height: u32) -> Result<Self, String> {
        let (w, h) = (width.saturating_as::<i32>(), height.saturating_as::<i32>());
        // SAFETY: the context is current; the objects are created here and unbound before returning.
        unsafe {
            let texture = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, param(glow::LINEAR));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, param(glow::LINEAR));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, param(glow::CLAMP_TO_EDGE));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, param(glow::CLAMP_TO_EDGE));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                param(glow::RGBA8),
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
            let framebuffer = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            if status != glow::FRAMEBUFFER_COMPLETE {
                gl.delete_framebuffer(framebuffer);
                gl.delete_texture(texture);
                return Err(format!("framebuffer incomplete ({status:#x})"));
            }
            Ok(Self {
                texture,
                framebuffer,
                width,
                height,
            })
        }
    }

    /// The framebuffer name mpv renders into.
    pub fn fbo(&self) -> i32 {
        self.framebuffer.0.get().saturating_as::<i32>()
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Whether the texture holds more than one color: reads a row through the middle (a check of the self-test, never per
    /// frame).
    pub fn has_content(&self, gl: &glow::Context) -> bool {
        let width = self.width.saturating_as::<usize>();
        let mut row = vec![0u8; width * 4];
        // SAFETY: the context is current; the framebuffer is complete and `row` holds one RGBA row of it.
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.framebuffer));
            gl.read_pixels(
                0,
                (self.height / 2).saturating_as::<i32>(),
                self.width.saturating_as::<i32>(),
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut row)),
            );
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
        }
        let (pixels, _) = row.as_chunks::<4>();
        pixels.iter().any(|pixel| Some(pixel) != pixels.first())
    }

    pub fn delete(self, gl: &glow::Context) {
        // SAFETY: the objects were created on this context or one of its share group, which is current.
        unsafe {
            gl.delete_framebuffer(self.framebuffer);
            gl.delete_texture(self.texture);
        }
    }
}

/// The internal format of the texture, as mpv wants it with the framebuffer (`MPV_RENDER_PARAM_OPENGL_FBO`).
pub const INTERNAL_FORMAT: i32 = 0x8058; // GL_RGBA8
