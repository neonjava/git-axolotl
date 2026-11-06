//! Encodes rendered `AxolotlFrame`s into crisp 2x-scaled looping `.gif`, static `.png`,
//! vector `.svg`, and `preview.html` (identical to `git-koi`'s output pipeline).

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::render::{
    AxolotlFrame, FRAMES_PER_SECOND, FRAME_HEIGHT_PIXELS, FRAME_WIDTH_PIXELS,
    SHARE_CARD_HEIGHT_PIXELS, SHARE_CARD_WIDTH_PIXELS,
};

/// Each native pixel becomes `2x2` exported pixels so the pixel art stays razor-sharp.
pub const EXPORT_SCALE: u32 = 2;
pub const EXPORT_WIDTH: u32 = FRAME_WIDTH_PIXELS * EXPORT_SCALE;
pub const EXPORT_HEIGHT: u32 = FRAME_HEIGHT_PIXELS * EXPORT_SCALE;

/// Share card scale (320x160 * 4 = 1280x640, exact GitHub Social Preview & X/LinkedIn card dimensions).
pub const SHARE_CARD_SCALE: u32 = 4;
pub const SHARE_CARD_EXPORT_WIDTH: u32 = SHARE_CARD_WIDTH_PIXELS * SHARE_CARD_SCALE;
pub const SHARE_CARD_EXPORT_HEIGHT: u32 = SHARE_CARD_HEIGHT_PIXELS * SHARE_CARD_SCALE;

const GIF_COLOR_LIMIT: usize = 255;
const GIF_TRANSPARENT_INDEX: u8 = 0;

/// Encode a static PNG by repeating each native pixel `EXPORT_SCALE` times in X and Y.
pub fn encode_png(frame: &AxolotlFrame) -> Result<Vec<u8>, String> {
    encode_scaled_rgba_png(
        &frame.rgba_pixels,
        FRAME_WIDTH_PIXELS,
        FRAME_HEIGHT_PIXELS,
        EXPORT_SCALE,
    )
}

/// Encode a 1280x640 social preview & annual evolution share card PNG from a 320x160 pixel buffer.
pub fn encode_share_card_png(native_pixels: &[[u8; 4]]) -> Result<Vec<u8>, String> {
    encode_scaled_rgba_png(
        native_pixels,
        SHARE_CARD_WIDTH_PIXELS,
        SHARE_CARD_HEIGHT_PIXELS,
        SHARE_CARD_SCALE,
    )
}

fn encode_scaled_rgba_png(
    native_pixels: &[[u8; 4]],
    native_w: u32,
    native_h: u32,
    scale: u32,
) -> Result<Vec<u8>, String> {
    let out_w = native_w * scale;
    let out_h = native_h * scale;
    let mut export_pixels = vec![[0_u8; 4]; (out_w * out_h) as usize];
    for (idx, px) in export_pixels.iter_mut().enumerate() {
        let nx = (idx as u32 % out_w) / scale;
        let ny = (idx as u32 / out_w) / scale;
        *px = native_pixels[(ny * native_w + nx) as usize];
    }

    let mut png_bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_bytes, out_w, out_h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    let flat: Vec<u8> = export_pixels.into_iter().flatten().collect();
    writer.write_image_data(&flat).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(png_bytes)
}

/// Encode a seamless animated GIF at `EXPORT_WIDTH x EXPORT_HEIGHT` repeating forever at `FRAMES_PER_SECOND`.
pub fn encode_gif(frames: &[AxolotlFrame]) -> Result<Vec<u8>, String> {
    if frames.is_empty() {
        return Err("GIF requires at least one frame".to_owned());
    }

    let mut gif_bytes = Vec::new();
    {
        let mut encoder = gif::Encoder::new(
            &mut gif_bytes,
            EXPORT_WIDTH as u16,
            EXPORT_HEIGHT as u16,
            &[],
        )
        .map_err(|e| e.to_string())?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(|e| e.to_string())?;

        for (frame_index, frame) in frames.iter().enumerate() {
            let [box_left, box_top, box_width, box_height] = find_drawn_box(&frame.rgba_pixels);
            let mut palette_indices: BTreeMap<[u8; 3], u8> = BTreeMap::new();
            let mut palette_channels = vec![0_u8; 3];
            let mut native_indices = Vec::with_capacity((box_width * box_height) as usize);

            for ny in box_top..box_top + box_height {
                for nx in box_left..box_left + box_width {
                    let [r, g, b, a] = frame.rgba_pixels[(ny * FRAME_WIDTH_PIXELS + nx) as usize];
                    let pal_idx = if a == 0 {
                        GIF_TRANSPARENT_INDEX
                    } else if let Some(&idx) = palette_indices.get(&[r, g, b]) {
                        idx
                    } else if palette_indices.len() < GIF_COLOR_LIMIT {
                        let idx = palette_indices.len() as u8 + 1;
                        palette_indices.insert([r, g, b], idx);
                        palette_channels.extend([r, g, b]);
                        idx
                    } else {
                        1
                    };
                    native_indices.push(pal_idx);
                }
            }

            let mut export_indices =
                Vec::with_capacity(native_indices.len() * (EXPORT_SCALE * EXPORT_SCALE) as usize);
            for native_row in native_indices.chunks(box_width as usize) {
                let mut export_row = Vec::with_capacity((box_width * EXPORT_SCALE) as usize);
                for &idx in native_row {
                    for _ in 0..EXPORT_SCALE {
                        export_row.push(idx);
                    }
                }
                for _ in 0..EXPORT_SCALE {
                    export_indices.extend_from_slice(&export_row);
                }
            }

            let delay = elapsed_hundredths(frame_index + 1) - elapsed_hundredths(frame_index);
            let gif_frame = gif::Frame {
                delay,
                dispose: gif::DisposalMethod::Background,
                transparent: Some(GIF_TRANSPARENT_INDEX),
                left: (box_left * EXPORT_SCALE) as u16,
                top: (box_top * EXPORT_SCALE) as u16,
                width: (box_width * EXPORT_SCALE) as u16,
                height: (box_height * EXPORT_SCALE) as u16,
                palette: Some(palette_channels),
                buffer: Cow::Owned(export_indices),
                ..gif::Frame::default()
            };
            encoder.write_frame(&gif_frame).map_err(|e| e.to_string())?;
        }
    }
    Ok(gif_bytes)
}

fn find_drawn_box(pixels: &[[u8; 4]]) -> [u32; 4] {
    let mut left = FRAME_WIDTH_PIXELS;
    let mut top = FRAME_HEIGHT_PIXELS;
    let mut right = 0_u32;
    let mut bottom = 0_u32;

    for (idx, px) in pixels.iter().enumerate() {
        if px[3] != 0 {
            let x = idx as u32 % FRAME_WIDTH_PIXELS;
            let y = idx as u32 / FRAME_WIDTH_PIXELS;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }

    if left > right {
        [0, 0, 1, 1]
    } else {
        [left, top, right + 1 - left, bottom + 1 - top]
    }
}

fn elapsed_hundredths(frame_index: usize) -> u16 {
    let fps = FRAMES_PER_SECOND as usize;
    ((frame_index * 200 + fps) / (2 * fps)) as u16
}

/// Build an HTML preview page showing `axolotl.gif` and `axolotl.png` on GitHub Light and Dark backgrounds.
pub fn encode_preview_html(caption: &str) -> String {
    let escaped = caption
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>git-axolotl preview</title>
<style>
  body {{ margin: 0; font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; }}
  h1, h2 {{ margin: 0; font-size: 14px; font-weight: 600; }}
  header, section {{ display: grid; gap: 12px; padding: 16px; }}
  .light {{ background: #ffffff; color: #1f2328; }}
  .dark {{ background: #0d1117; color: #f0f6fc; }}
  .images {{ display: flex; flex-wrap: wrap; gap: 16px; }}
  figure {{ margin: 0; }}
  img {{ display: block; max-width: 100%; height: auto; image-rendering: pixelated; }}
</style>
</head>
<body>
<header class="dark"><h1>{escaped}</h1></header>
<section class="dark">
<h2>GitHub Dark Mode</h2>
<div class="images">
<figure><img src="axolotl.gif" width="{EXPORT_WIDTH}" height="{EXPORT_HEIGHT}" alt="The Axolotl swimming"><figcaption>axolotl.gif (Animated 15 FPS)</figcaption></figure>
<figure><img src="axolotl.png" width="{EXPORT_WIDTH}" height="{EXPORT_HEIGHT}" alt="The Axolotl still"><figcaption>axolotl.png (Static Still)</figcaption></figure>
</div>
</section>
<section class="light">
<h2>GitHub Light Mode</h2>
<div class="images">
<figure><img src="axolotl.gif" width="{EXPORT_WIDTH}" height="{EXPORT_HEIGHT}" alt="The Axolotl swimming"><figcaption>axolotl.gif (Animated 15 FPS)</figcaption></figure>
<figure><img src="axolotl.png" width="{EXPORT_WIDTH}" height="{EXPORT_HEIGHT}" alt="The Axolotl still"><figcaption>axolotl.png (Static Still)</figcaption></figure>
</div>
</section>
</body>
</html>
"#
    )
}
