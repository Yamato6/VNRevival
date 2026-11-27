//! Software compositing: a fixed-size RGBA framebuffer and sprites.
//!
//! Old visual novels draw at a fixed resolution (Pure Mail: 800 × 600)
//! with simple alpha blending, so the whole frame is composed on the CPU
//! and handed to the frontend as one texture. That keeps rendering
//! identical on every platform and makes screenshots in tests exact.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// An 8-bit RGBA colour (straight alpha).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// The same colour with its alpha multiplied by `opacity / 255`.
    pub fn with_opacity(self, opacity: u8) -> Self {
        Self {
            a: mul8(self.a, opacity),
            ..self
        }
    }
}

/// `a * b / 255`, rounded.
fn mul8(a: u8, b: u8) -> u8 {
    let x = u32::from(a) * u32::from(b) + 128;
    ((x + (x >> 8)) >> 8) as u8
}

/// Blends one channel: `src * alpha + dst * (255 - alpha)`, rounded.
fn blend8(dst: u8, src: u8, alpha: u8) -> u8 {
    let x = u32::from(src) * u32::from(alpha) + u32::from(dst) * u32::from(255 - alpha) + 128;
    ((x + (x >> 8)) >> 8) as u8
}

/// An image with straight alpha, drawn onto a [`Canvas`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, `R, G, B, A`, rows top-down.
    pub pixels: Vec<u8>,
}

impl Sprite {
    /// From tightly packed `R, G, B` pixels (fully opaque).
    pub fn from_rgb(width: u32, height: u32, rgb: &[u8]) -> Self {
        assert_eq!(rgb.len(), width as usize * height as usize * 3);
        let pixels = rgb
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }

    /// From tightly packed `R, G, B, A` pixels.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        assert_eq!(rgba.len(), width as usize * height as usize * 4);
        Self {
            width,
            height,
            pixels: rgba,
        }
    }

    /// From an 8-bit mask: every pixel is `color`, with the mask as alpha.
    pub fn from_mask(width: u32, height: u32, mask: &[u8], color: Color) -> Self {
        assert_eq!(mask.len(), width as usize * height as usize);
        let pixels = mask
            .iter()
            .flat_map(|&a| [color.r, color.g, color.b, mul8(a, color.a)])
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }
}

/// The frame being composed. Always opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    width: u32,
    height: u32,
    /// `R, G, B, A` with `A = 255`, rows top-down.
    pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: [0, 0, 0, 255].repeat(width as usize * height as usize),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels as `R, G, B, A` bytes, rows top-down.
    pub fn rgba(&self) -> &[u8] {
        &self.pixels
    }

    /// Colour of one pixel.
    pub fn pixel(&self, x: u32, y: u32) -> Color {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = &self.pixels[i..i + 4];
        Color::rgba(p[0], p[1], p[2], p[3])
    }

    pub fn clear(&mut self, color: Color) {
        for px in self.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&[color.r, color.g, color.b, 255]);
        }
    }

    /// Clips a rectangle at `(x, y)` to the canvas. Returns the visible
    /// part as `(dst_x, dst_y, src_x, src_y, width, height)`.
    fn clip(
        &self,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    ) -> Option<(usize, usize, usize, usize, usize, usize)> {
        let x0 = i64::from(x).max(0);
        let y0 = i64::from(y).max(0);
        let x1 = (i64::from(x) + i64::from(w)).min(i64::from(self.width));
        let y1 = (i64::from(y) + i64::from(h)).min(i64::from(self.height));
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some((
            x0 as usize,
            y0 as usize,
            (x0 - i64::from(x)) as usize,
            (y0 - i64::from(y)) as usize,
            (x1 - x0) as usize,
            (y1 - y0) as usize,
        ))
    }

    /// Fills a rectangle, blending by the colour's alpha.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Color) {
        let Some((dx, dy, _, _, cw, ch)) = self.clip(x, y, w, h) else {
            return;
        };
        let stride = self.width as usize * 4;
        for row in dy..dy + ch {
            let start = row * stride + dx * 4;
            for px in self.pixels[start..start + cw * 4].chunks_exact_mut(4) {
                px[0] = blend8(px[0], color.r, color.a);
                px[1] = blend8(px[1], color.g, color.a);
                px[2] = blend8(px[2], color.b, color.a);
            }
        }
    }

    /// Draws a sprite with its top-left corner at `(x, y)`, its alpha
    /// further scaled by `opacity` (255 = as is).
    pub fn draw(&mut self, sprite: &Sprite, x: i32, y: i32, opacity: u8) {
        if opacity == 0 {
            return;
        }
        let Some((dx, dy, sx, sy, cw, ch)) = self.clip(x, y, sprite.width, sprite.height) else {
            return;
        };
        let dst_stride = self.width as usize * 4;
        let src_stride = sprite.width as usize * 4;
        for row in 0..ch {
            let d = (dy + row) * dst_stride + dx * 4;
            let s = (sy + row) * src_stride + sx * 4;
            let dst = &mut self.pixels[d..d + cw * 4];
            let src = &sprite.pixels[s..s + cw * 4];
            for (dp, sp) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                let alpha = if opacity == 255 {
                    sp[3]
                } else {
                    mul8(sp[3], opacity)
                };
                match alpha {
                    0 => {}
                    255 => dp[..3].copy_from_slice(&sp[..3]),
                    a => {
                        dp[0] = blend8(dp[0], sp[0], a);
                        dp[1] = blend8(dp[1], sp[1], a);
                        dp[2] = blend8(dp[2], sp[2], a);
                    }
                }
            }
        }
    }

    /// Draws an 8-bit coverage mask (`width * height` bytes) in `color`
    /// with its top-left corner at `(x, y)`. Used for text.
    pub fn draw_mask(
        &mut self,
        width: u32,
        height: u32,
        mask: &[u8],
        x: i32,
        y: i32,
        color: Color,
    ) {
        let Some((dx, dy, sx, sy, cw, ch)) = self.clip(x, y, width, height) else {
            return;
        };
        let stride = self.width as usize * 4;
        for row in 0..ch {
            let d = (dy + row) * stride + dx * 4;
            let s = (sy + row) * width as usize + sx;
            let dst = &mut self.pixels[d..d + cw * 4];
            for (dp, &coverage) in dst.chunks_exact_mut(4).zip(&mask[s..s + cw]) {
                let a = mul8(coverage, color.a);
                if a != 0 {
                    dp[0] = blend8(dp[0], color.r, a);
                    dp[1] = blend8(dp[1], color.g, a);
                    dp[2] = blend8(dp[2], color.b, a);
                }
            }
        }
    }

    /// Draws `other` (same size) over this canvas at `opacity`; used for
    /// cross-fades.
    pub fn blend_from(&mut self, other: &Canvas, opacity: u8) {
        assert_eq!((self.width, self.height), (other.width, other.height));
        if opacity == 255 {
            self.pixels.copy_from_slice(&other.pixels);
            return;
        }
        for (d, s) in self
            .pixels
            .chunks_exact_mut(4)
            .zip(other.pixels.chunks_exact(4))
        {
            d[0] = blend8(d[0], s[0], opacity);
            d[1] = blend8(d[1], s[1], opacity);
            d[2] = blend8(d[2], s[2], opacity);
        }
    }

    /// Writes the canvas as an RGB PNG.
    pub fn save_png(&self, path: &Path) -> std::io::Result<()> {
        let file = BufWriter::new(File::create(path)?);
        let mut encoder = png::Encoder::new(file, self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
        let rgb: Vec<u8> = self
            .pixels
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        writer
            .write_image_data(&rgb)
            .map_err(std::io::Error::other)?;
        writer.finish().map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blending_math() {
        assert_eq!(mul8(255, 255), 255);
        assert_eq!(mul8(255, 0), 0);
        assert_eq!(mul8(128, 255), 128);
        assert_eq!(blend8(0, 255, 128), 128);
        assert_eq!(blend8(200, 100, 0), 200);
        assert_eq!(blend8(200, 100, 255), 100);
    }

    #[test]
    fn draws_clipped_sprites_with_alpha() {
        let mut canvas = Canvas::new(4, 2);
        canvas.clear(Color::rgb(0, 0, 100));
        let sprite = Sprite::from_rgba(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 128]);
        canvas.draw(&sprite, 3, 1, 255); // only the first pixel is visible
        assert_eq!(canvas.pixel(3, 1), Color::rgb(255, 0, 0));
        canvas.draw(&sprite, -1, 0, 255); // only the second pixel
        assert_eq!(canvas.pixel(0, 0), Color::rgb(0, 128, 50));
        canvas.draw(&sprite, 100, 100, 255); // off-canvas: no panic
        canvas.draw(&sprite, 0, 1, 0); // invisible
        assert_eq!(canvas.pixel(0, 1), Color::rgb(0, 0, 100));
    }

    #[test]
    fn fills_and_crossfades() {
        let mut a = Canvas::new(2, 2);
        a.fill_rect(1, 1, 5, 5, Color::rgba(255, 255, 255, 255));
        assert_eq!(a.pixel(1, 1), Color::WHITE);
        assert_eq!(a.pixel(0, 0), Color::BLACK);
        let mut b = Canvas::new(2, 2);
        b.clear(Color::WHITE);
        a.blend_from(&b, 128);
        assert_eq!(a.pixel(0, 0), Color::rgb(128, 128, 128));
        assert_eq!(a.pixel(1, 1), Color::WHITE);
    }

    #[test]
    fn masks_become_coloured_sprites() {
        let s = Sprite::from_mask(2, 1, &[0, 255], Color::rgba(10, 20, 30, 128));
        assert_eq!(s.pixels, [10, 20, 30, 0, 10, 20, 30, 128]);
    }
}
