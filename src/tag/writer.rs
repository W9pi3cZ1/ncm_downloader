use std::path::Path;

use lofty::{
    TextEncoding,
    config::WriteOptions,
    file::{AudioFile, TaggedFileExt},
    id3::v2::{Id3v2Tag, UnsynchronizedTextFrame},
    picture::{MimeType, Picture, PictureType},
    probe::Probe,
    tag::{Accessor, ItemKey, ItemValue, Tag, TagItem, TagType},
};

/// 一首歌落盘后要写入的元数据。
pub struct TrackTags<'a> {
    pub title: &'a str,
    pub artists: &'a [String],
    pub album: &'a str,
    pub album_artists: &'a [String],
    pub track: u32,
    pub track_total: u32,
    pub disc: u32,
    pub disc_total: u32,
    pub disc_subtitle: Option<&'a str>,
    pub release_date: &'a str, // "YYYY-MM-DD"
    pub copyright: Option<&'a str>,
    pub lyrics: Option<&'a str>,
    pub description: Option<&'a str>,
    pub cover: Option<(&'a [u8], Option<MimeType>)>,
}

pub fn write_to(path: &Path, t: &TrackTags<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = Probe::open(path)?.read()?;
    if file.primary_tag().is_none() {
        let ty = file.primary_tag_type();
        file.insert_tag(Tag::new(ty));
    }
    let tag = file.primary_tag_mut().ok_or("no primary tag")?;

    // 抹掉可能残留的封面（重复 tag 时很关键）
    while !tag.pictures().is_empty() {
        tag.remove_picture(0);
    }

    tag.set_title(t.title.to_owned());
    tag.set_album(t.album.to_owned());

    match tag.tag_type() {
        TagType::VorbisComments => {
            tag.remove_key(ItemKey::TrackArtist);
            tag.remove_key(ItemKey::TrackArtists);
            for a in t.artists {
                tag.push(TagItem::new(
                    ItemKey::TrackArtist,
                    ItemValue::Text(a.clone()),
                ));
                tag.push(TagItem::new(
                    ItemKey::TrackArtists,
                    ItemValue::Text(a.clone()),
                ));
            }
            tag.remove_key(ItemKey::AlbumArtist);
            tag.remove_key(ItemKey::AlbumArtists);
            for a in t.album_artists {
                tag.push(TagItem::new(
                    ItemKey::AlbumArtist,
                    ItemValue::Text(a.clone()),
                ));
                tag.push(TagItem::new(
                    ItemKey::AlbumArtists,
                    ItemValue::Text(a.clone()),
                ));
            }
        }
        _ => {
            tag.set_artist(t.artists.join("; "));
            if !t.album_artists.is_empty() {
                tag.insert_text(ItemKey::AlbumArtist, t.album_artists.join("; "));
            }
        }
    }

    if t.track > 0 {
        tag.set_track(t.track);
    }
    if t.disc > 0 {
        tag.set_disk(t.disc);
    }
    if let Some(s) = t.disc_subtitle {
        tag.insert_text(ItemKey::SetSubtitle, s.to_owned());
    }
    if t.track_total > 0 {
        tag.insert_text(ItemKey::TrackTotal, t.track_total.to_string());
    }
    if t.disc_total > 1 {
        tag.insert_text(ItemKey::DiscTotal, t.disc_total.to_string());
    }
    tag.insert_text(ItemKey::RecordingDate, t.release_date.to_owned());
    tag.insert_text(ItemKey::ReleaseDate, t.release_date.to_owned());
    if let Some(c) = t.copyright {
        tag.insert_text(ItemKey::CopyrightMessage, c.to_owned());
    }
    if let Some(l) = t.lyrics {
        // 只有主标签是 ID3v2 时才需要特殊处理
        if tag.tag_type() == TagType::Id3v2 {
            // 1. 将通用的 Tag 克隆并转换为 Id3v2Tag
            let mut id3v2 = Id3v2Tag::from(tag.clone());

            // 3. 构造并插入新的 USLT 帧
            let frame = UnsynchronizedTextFrame::new(
                TextEncoding::UTF8, // 推荐使用 UTF-8
                *b"XXX",            // 指定语言代码，不知道所以XXX
                String::new(),      // 描述留空或设置一个唯一值
                l.to_owned(),
            );
            id3v2.insert(frame.into());

            // 4. 转换回 Tag 并覆盖原来的主标签
            *tag = Tag::from(id3v2);
        } else {
            // 非 ID3v2 格式（如 Vorbis Comments）继续用通用的 UnsyncLyrics
            tag.insert_text(ItemKey::UnsyncLyrics, l.to_owned());
        }
    }
    if let Some(d) = t.description {
        tag.insert_text(ItemKey::Description, d.to_owned());
    }

    if let Some((data, ref mime)) = t.cover {
        let mut pic = Picture::unchecked(data.to_vec()).pic_type(PictureType::CoverFront);
        if let Some(m) = mime {
            pic = pic.mime_type(m.clone());
        }
        tag.push_picture(pic.build());
    }

    file.save_to_path(path, WriteOptions::default())?;
    Ok(())
}
