use lofty::picture::MimeType;

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
