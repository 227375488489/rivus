//! Rasterisation of a laid-out document with `tiny-skia`.

use std::collections::HashMap;

use cosmic_text::{Color as TextColor, FontSystem, SwashCache, SwashContent};
use image::{imageops::FilterType, RgbaImage};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect as SkRect, Transform};

use crate::layout::{Layout, Rect};
use crate::style::Rgba;

/// Decoded image cache shared across renders of the same page.
pub type ImageCache = HashMap<String, Option<RgbaImage>>;

fn sk_color(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c[0], c[1], c[2], c[3])
}

fn rounded_path(rect: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let r = radius.min(rect.w / 2.0).min(rect.h / 2.0).max(0.0);
    if r <= 0.5 {
        let rect = SkRect::from_xywh(rect.x, rect.y, rect.w, rect.h)?;
        return Some(PathBuilder::from_rect(rect));
    }
    let (x, y, w, h) = (rect.x, rect.y, rect.w, rect.h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill_rect(pixmap: &mut Pixmap, rect: Rect, color: Rgba, radius: f32) {
    if color[3] == 0
        || !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.w.is_finite()
        || !rect.h.is_finite()
        || rect.w <= 0.0
        || rect.h <= 0.0
    {
        return;
    }
    let pw = pixmap.width() as f32;
    let ph = pixmap.height() as f32;
    let x0 = rect.x.max(0.0);
    let y0 = rect.y.max(0.0);
    let x1 = (rect.x + rect.w).min(pw);
    let y1 = (rect.y + rect.h).min(ph);
    if x1 - x0 < 0.5 || y1 - y0 < 0.5 {
        return;
    }
    let rect = Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    };
    let mut paint = Paint::default();
    paint.set_color(sk_color(color));
    paint.anti_alias = true;
    if radius > 0.5 && radius.is_finite() {
        if let Some(path) = rounded_path(rect, radius) {
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    } else {
        if let Some(r) = SkRect::from_xywh(rect.x.round(), rect.y.round(), rect.w.round(), rect.h.round()) {
            if r.width() >= 1.0 && r.height() >= 1.0 {
                pixmap.fill_rect(r, &paint, Transform::identity(), None);
            }
        }
    }
}

fn blend_pixel(pixmap: &mut Pixmap, x: i32, y: i32, color: TextColor, coverage: u8) {
    let w = pixmap.width() as i32;
    let h = pixmap.height() as i32;
    if x < 0 || y < 0 || x >= w || y >= h {
        return;
    }
    let a = (coverage as u32 * color.a() as u32) / 255;
    if a == 0 {
        return;
    }
    let idx = (y as u32 * pixmap.width() + x as u32) as usize;
    let data = pixmap.data_mut();
    let dst = &mut data[idx * 4..idx * 4 + 4];
    let inv = 255 - a;
    dst[0] = ((color.r() as u32 * a + dst[0] as u32 * inv) / 255) as u8;
    dst[1] = ((color.g() as u32 * a + dst[1] as u32 * inv) / 255) as u8;
    dst[2] = ((color.b() as u32 * a + dst[2] as u32 * inv) / 255) as u8;
    dst[3] = 255;
}

fn draw_image(pixmap: &mut Pixmap, rect: Rect, src: &RgbaImage) {
    let w = rect.w.round().max(1.0) as u32;
    let h = rect.h.round().max(1.0) as u32;
    let scaled = image::imageops::resize(src, w, h, FilterType::Triangle);
    let ox = rect.x.round() as i32;
    let oy = rect.y.round() as i32;
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let stride = pixmap.width();
    let data = pixmap.data_mut();
    for y in 0..h as i32 {
        let py = oy + y;
        if py < 0 || py >= ph {
            continue;
        }
        for x in 0..w as i32 {
            let px = ox + x;
            if px < 0 || px >= pw {
                continue;
            }
            let s = scaled.get_pixel(x as u32, y as u32).0;
            let a = s[3] as u32;
            if a == 0 {
                continue;
            }
            let idx = ((py as u32 * stride + px as u32) * 4) as usize;
            let inv = 255 - a;
            data[idx] = ((s[0] as u32 * a + data[idx] as u32 * inv) / 255) as u8;
            data[idx + 1] = ((s[1] as u32 * a + data[idx + 1] as u32 * inv) / 255) as u8;
            data[idx + 2] = ((s[2] as u32 * a + data[idx + 2] as u32 * inv) / 255) as u8;
            data[idx + 3] = 255;
        }
    }
}

/// Paint a layout into an RGBA pixmap of `width` x `height` device pixels.
pub fn paint(
    layout: &Layout,
    width: u32,
    height: u32,
    font_system: &mut FontSystem,
    swash: &mut SwashCache,
    images: &mut ImageCache,
) -> Option<Pixmap> {
    let width = width.max(1);
    let height = height.max(1).min(24_000);
    let mut pixmap = Pixmap::new(width, height)?;
    pixmap.fill(sk_color(layout.page_bg));

    for fill in &layout.fills {
        fill_rect(&mut pixmap, fill.rect, fill.color, fill.radius);
    }

    for image_box in &layout.images {
        let decoded = images
            .entry(image_box.url.clone())
            .or_insert_with(|| {
                image_box
                    .bytes
                    .as_ref()
                    .and_then(|b| image::load_from_memory(b).ok().map(|i| i.to_rgba8()))
            });
        match decoded {
            Some(img) => draw_image(&mut pixmap, image_box.rect, img),
            None => fill_rect(
                &mut pixmap,
                image_box.rect,
                [0, 0, 0, 12],
                layout.fills.first().map(|f| f.radius).unwrap_or(0.0),
            ),
        }
    }

    for layer in &layout.texts {
        for run in layer.buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let phys = glyph.physical((layer.x, layer.y + run.line_y), 1.0);
                let color = glyph.color_opt.unwrap_or(TextColor::rgb(26, 27, 30));
                let meta = layer.meta.get(glyph.metadata).cloned().unwrap_or_default();
                let Some(img) = swash.get_image(font_system, phys.cache_key).as_ref() else {
                    continue;
                };
                let gx = phys.x + img.placement.left;
                let gy = phys.y - img.placement.top;
                let w = img.placement.width as i32;
                let h = img.placement.height as i32;
                if w <= 0 || h <= 0 {
                    continue;
                }
                for row in 0..h {
                    for col in 0..w {
                        match img.content {
                            SwashContent::Mask => {
                                let idx = (row * w + col) as usize;
                                if let Some(a) = img.data.get(idx) {
                                    if *a > 0 {
                                        blend_pixel(&mut pixmap, gx + col, gy + row, color, *a);
                                    }
                                }
                            }
                            SwashContent::Color => {
                                let idx = ((row * w + col) * 4) as usize;
                                if idx + 3 < img.data.len() {
                                    let a = img.data[idx + 3];
                                    if a > 0 {
                                        let c = TextColor::rgba(
                                            img.data[idx],
                                            img.data[idx + 1],
                                            img.data[idx + 2],
                                            a,
                                        );
                                        blend_pixel(&mut pixmap, gx + col, gy + row, c, a);
                                    }
                                }
                            }
                            SwashContent::SubpixelMask => {}
                        }
                    }
                }
                if meta.underline || meta.strike {
                    let thickness = (glyph.font_size / 14.0).max(1.0);
                    let width = (img.placement.width as f32).max(glyph.font_size * 0.5);
                    if meta.underline {
                        fill_rect(
                            &mut pixmap,
                            Rect::new(gx as f32, phys.y as f32 + glyph.font_size * 0.1, width, thickness),
                            [color.r(), color.g(), color.b(), 210],
                            0.0,
                        );
                    }
                    if meta.strike {
                        fill_rect(
                            &mut pixmap,
                            Rect::new(gx as f32, phys.y as f32 - glyph.font_size * 0.28, width, thickness),
                            [color.r(), color.g(), color.b(), 210],
                            0.0,
                        );
                    }
                }
            }
        }
    }

    Some(pixmap)
}
