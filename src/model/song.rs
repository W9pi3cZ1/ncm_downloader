use std::time::{Duration, SystemTime};

use url::Url;

use crate::{
    model::{AlbumId, ArtistRef, SongId, lyric::Lyrics},
    ncm::{AudioQuality, client::Timed, dto},
    util::PrettyUrl,
};

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct SongUrl {
    pub level: AudioQuality,
    pub url: PrettyUrl,
    pub fetched_at: SystemTime,
    pub expires: Duration,
}

impl From<Timed<dto::song_url::Payload>> for SongUrl {
    fn from(value: Timed<dto::song_url::Payload>) -> Self {
        Self {
            level: value.value.level,
            url: Url::parse(&value.value.url).unwrap().into(),
            fetched_at: value.timing.sent_at.clone(),
            expires: Duration::from_secs(value.value.expi),
        }
    }
}

#[allow(unused)]
impl SongUrl {
    /// TODO: 实现过期处理和音乐库导出
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now();
        // 留出30s时间
        now >= (self.fetched_at + self.expires - Duration::from_secs(30))
    }
}

// #[derive(Debug)]
// pub struct Privilege {
//     pub pl_level: AudioQuality,
//     pub dl_level: AudioQuality,
// }

#[derive(Debug)]
pub struct SongDisc {
    pub curr: u32,
    pub curr_track: u32,
    pub total: u32,
    pub subtitle: Option<String>,
    pub track_total: u32,
}

#[allow(unused)]
#[derive(Debug)]
pub struct Song {
    pub id: SongId,
    pub name: String,
    pub artists: Vec<ArtistRef>,
    pub album: AlbumId,
    pub pic_url: PrettyUrl, // 有时存在与专封不一致的单曲封面
    pub lyrics: Lyrics,
    pub discs: SongDisc,
    pub url: Option<SongUrl>,
}

// impl From<dto::album::SongDetail> for Song {
//     fn from(value: dto::album::SongDetail) -> Self {
//         Self {
//             id: SongId(value.id),
//             name: value.name,
//             artists: value
//                 .ar
//                 .iter()
//                 .map(|x| ArtistRef::new(x.id, x.name.clone()))
//                 .collect(),
//             album: AlbumId(0), // set by push_album
//             pic_url: String::new(), // set by push_album
//             lyrics: Lyrics::empty(), // set by push_album
//             url: None, // set by push_album
//         }
//     }
// }
