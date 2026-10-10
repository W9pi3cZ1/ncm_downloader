use std::{collections::HashMap, sync::Mutex};

use lofty::picture::MimeType;
use url::Url;

use crate::model::downloader::AlbumResources;

pub fn detect(data: &[u8]) -> Option<MimeType> {
    let kind = infer::get(data)?;
    if kind.matcher_type() != infer::MatcherType::Image {
        return None;
    }
    Some(match kind.mime_type() {
        "image/png" => MimeType::Png,
        "image/jpeg" => MimeType::Jpeg,
        "image/gif" => MimeType::Gif,
        "image/bmp" => MimeType::Bmp,
        "image/tiff" => MimeType::Tiff,
        other => MimeType::Unknown(other.to_owned()),
    })
}

/* ==================== 封面压缩 ==================== */

/// 嵌入用封面缓存：url.path() -> 处理后的字节（None = 放弃嵌入）
#[derive(Default)]
pub struct CoverEmbedCache(Mutex<HashMap<String, Option<Vec<u8>>>>);

impl CoverEmbedCache {
    pub fn get_or_prepare(&self, resources: &Mutex<AlbumResources>, url: &Url) -> Option<Vec<u8>> {
        let key = url.path().to_owned();

        // 命中直接返回
        if let Some(cached) = self.0.lock().unwrap().get(&key) {
            return cached.clone();
        }

        // 未命中：读原图 → 压缩 → 入缓存
        let raw = {
            let res = resources.lock().unwrap();
            res.get(url).and_then(|p| std::fs::read(p).ok())
        };
        let prepared = raw.and_then(prepare_cover);
        self.0.lock().unwrap().insert(key, prepared.clone());
        prepared
    }
}

/// FLAC `METADATA_BLOCK_PICTURE` 上限 ~16 MiB，留 1 MiB 余量
pub const MAX_EMBED_SIZE: usize = 15 * 1024 * 1024;
/// 低于此大小直接嵌原图，不浪费 CPU
pub const DIRECT_EMBED_THRESHOLD: usize = 1024 * 1024;
/// 压缩目标：最长边、JPEG 质量
pub const COVER_MAX_SIDE: u32 = 1200;
pub const COVER_JPEG_QUALITY: u8 = 85;

/// 生成适合嵌入的封面字节。
/// - 原图小 → 直接用
/// - 原图大 → 缩放 + 重编码
/// - 压缩失败 / 仍超限 → None（调用方跳过嵌入）
pub fn prepare_cover(raw: Vec<u8>) -> Option<Vec<u8>> {
    if raw.len() <= DIRECT_EMBED_THRESHOLD {
        return Some(raw);
    }
    match compress_cover(&raw) {
        Ok(c) if c.len() <= MAX_EMBED_SIZE => Some(c),
        Ok(c) => {
            // never trigger it,,,
            tracing::warn!(
                "cover still {} MiB after compression, skip embed",
                c.len() / 1024 / 1024
            );
            None
        }
        Err(e) => {
            tracing::warn!("cover compression failed: {e}");
            if raw.len() <= MAX_EMBED_SIZE {
                Some(raw)
            } else {
                None
            }
        }
    }
}

/// 是否有真实 alpha（存在非 255/65535 的像素）
fn has_real_alpha(img: &image::DynamicImage) -> bool {
    use image::DynamicImage::*;
    match img {
        ImageRgba8(rgba) => rgba.as_raw().chunks_exact(4).any(|p| p[3] != 255),
        ImageRgba16(rgba) => rgba.as_raw().chunks_exact(4).any(|p| p[3] != u16::MAX),
        _ => false, // Rgb8 / Rgb16 / Luma 等根本没有 alpha
    }
}

fn compress_cover(raw: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use image::{ImageFormat, imageops::FilterType};
    use std::io::Cursor;

    let img = image::load_from_memory(raw)?;
    let (w, h) = (img.width(), img.height());
    let img = if w > COVER_MAX_SIDE || h > COVER_MAX_SIDE {
        img.resize(COVER_MAX_SIDE, COVER_MAX_SIDE, FilterType::Lanczos3)
    } else {
        img
    };

    // 只有真 alpha 才值得用 PNG
    if has_real_alpha(&img) {
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Png)?;
        let png = buf.into_inner();

        if png.len() <= MAX_EMBED_SIZE {
            return Ok(png);
        }
        tracing::warn!(
            "PNG still {} MiB after resize, falling back to JPEG (alpha will be dropped)",
            png.len() / 1024 / 1024
        );
        // 掉到下面走 JPEG 分支
    }

    // 无 alpha，或 PNG 超限 → JPEG
    let rgb = img.to_rgb8();
    let mut buf = Cursor::new(Vec::new());
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, COVER_JPEG_QUALITY);
    enc.encode_image(&rgb)?;
    Ok(buf.into_inner())
}
