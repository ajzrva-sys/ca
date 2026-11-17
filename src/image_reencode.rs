//! Downscale and re-encode a screenshot before it enters the conversation.
//! PNG is preferred. JPEG is used when the PNG is over the byte cap.

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};

const RGBA_BYTES_PER_PIXEL: usize = 4;
const RGB_BYTES_PER_PIXEL: usize = 3;
const MIN_EDGE: u32 = 1;
const JPEG_QUALITIES: [u8; 4] = [55, 35, 20, 10];
const SHRINK_STEPS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageEncoding {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reencoded {
    pub bytes: Vec<u8>,
    pub format: ImageEncoding,
    pub width: u32,
    pub height: u32,
}

/// Fit `width`/`height` to `max_edge`, then halve the long side until raw RGB
/// bytes fit in `max_bytes`.
pub fn plan_edge(width: u32, height: u32, max_edge: u32, max_bytes: usize) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (0, 0);
    }
    let max_edge = max_edge.max(MIN_EDGE);
    let mut w = width;
    let mut h = height;
    let long = w.max(h);
    if long > max_edge {
        w = scale_side(w, long, max_edge);
        h = scale_side(h, long, max_edge);
    }
    while w.max(h) > MIN_EDGE && rgb_bytes(w, h) > max_bytes {
        let long = w.max(h);
        let next = (long / 2).max(MIN_EDGE);
        if next >= long {
            break;
        }
        w = scale_side(w, long, next);
        h = scale_side(h, long, next);
    }
    (w.max(MIN_EDGE), h.max(MIN_EDGE))
}

/// Re-encode raw RGBA. The result's byte length is within `max_bytes` or an error.
pub fn reencode_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
    max_bytes: usize,
    max_edge: u32,
) -> Result<Reencoded, String> {
    let pixels = (width as usize).saturating_mul(height as usize);
    let expected = pixels.saturating_mul(RGBA_BYTES_PER_PIXEL);
    if width == 0 || height == 0 || expected != rgba.len() {
        return Err("rgba length does not match the image dimensions".to_string());
    }

    let mut edge_limit = max_edge.max(MIN_EDGE);
    let mut previous: Option<(u32, u32)> = None;
    for _ in 0..SHRINK_STEPS {
        let (w, h) = plan_edge(width, height, edge_limit, max_bytes);
        if w == 0 || h == 0 {
            break;
        }
        if previous == Some((w, h)) {
            break;
        }
        previous = Some((w, h));
        let rgb = resample(rgba, width, height, w, h);
        let png = encode_png(&rgb)?;
        if png.len() <= max_bytes {
            return Ok(Reencoded {
                bytes: png,
                format: ImageEncoding::Png,
                width: w,
                height: h,
            });
        }
        for quality in JPEG_QUALITIES {
            let jpeg = encode_jpeg(&rgb, quality)?;
            if jpeg.len() <= max_bytes {
                return Ok(Reencoded {
                    bytes: jpeg,
                    format: ImageEncoding::Jpeg,
                    width: w,
                    height: h,
                });
            }
        }
        let long = w.max(h);
        if long <= MIN_EDGE {
            break;
        }
        edge_limit = (long / 2).max(MIN_EDGE);
    }
    Err("encoded image does not fit the byte cap".to_string())
}

fn rgb_bytes(width: u32, height: u32) -> usize {
    (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(RGB_BYTES_PER_PIXEL)
}

fn scale_side(side: u32, long: u32, next_long: u32) -> u32 {
    let scaled = (u64::from(side) * u64::from(next_long)) / u64::from(long.max(1));
    scaled.max(u64::from(MIN_EDGE)) as u32
}

fn resample(rgba: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> RgbImage {
    let mut image = RgbImage::new(dst_w, dst_h);
    for y in 0..dst_h {
        let sy = ((u64::from(y) * u64::from(src_h)) / u64::from(dst_h)) as u32;
        let sy = sy.min(src_h - 1);
        for x in 0..dst_w {
            let sx = ((u64::from(x) * u64::from(src_w)) / u64::from(dst_w)) as u32;
            let sx = sx.min(src_w - 1);
            let index = (sy as usize * src_w as usize + sx as usize) * RGBA_BYTES_PER_PIXEL;
            let pixel = Rgb([rgba[index], rgba[index + 1], rgba[index + 2]]);
            image.put_pixel(x, y, pixel);
        }
    }
    image
}

fn encode_png(rgb: &RgbImage) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|err| err.to_string())?;
    Ok(bytes)
}

fn encode_jpeg(rgb: &RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut bytes, quality);
    encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|err| err.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_image_keeps_its_dimensions() {
        let width = 4u32;
        let height = 3u32;
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for (index, pixel) in rgba.chunks_mut(4).enumerate() {
            pixel[0] = (index * 30) as u8;
            pixel[1] = 40;
            pixel[2] = 90;
            pixel[3] = 255;
        }
        let cap = 64 * 1024;
        let encoded = reencode_rgba(&rgba, width, height, cap, 128).expect("encode");
        assert_eq!((encoded.width, encoded.height), (width, height));
        assert!(encoded.bytes.len() <= cap);
        let decoded = image::load_from_memory(&encoded.bytes).expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (width, height));
    }

    #[test]
    fn a_large_solid_image_shrinks_under_a_tight_cap() {
        let width = 320u32;
        let height = 240u32;
        let rgba = vec![12u8; (width * height * 4) as usize];
        let cap = 2_048usize;
        let encoded = reencode_rgba(&rgba, width, height, cap, 4_096).expect("encode");
        assert!(encoded.width < width, "{}", encoded.width);
        assert!(encoded.height < height, "{}", encoded.height);
        assert!(encoded.bytes.len() <= cap, "{}", encoded.bytes.len());
    }
}
