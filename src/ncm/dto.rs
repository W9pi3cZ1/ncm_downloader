#![allow(dead_code)]
pub mod album {
    use crate::ncm::AudioQuality;
    use serde::Deserialize;

    /**
     * url: /album?id={}
     */
    #[derive(Debug, Deserialize, Clone)]
    pub struct API {
        pub album: AlbumDetail,
        pub songs: Vec<SongDetail>,
        pub code: u16,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct AlbumDetail {
        pub id: u64,
        pub name: String, // META album
        #[serde(rename = "picUrl")]
        pub pic_url: String, // FILE cover.EXT
        #[serde(rename = "publishTime")]
        pub publish_time: u64, // META release_date (millseconds timestamp UTC)
        pub company: Option<String>, // META copyright ("" means Nothing)
        pub artists: Vec<AlbumArtist>, // META album_artist
        pub description: Option<String>, // META description
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct SongDetail {
        pub id: u64,
        pub name: String,
        pub ar: Vec<SongArtist>,
        pub no: u64,
        pub cd: String,
        pub privilege: Privilege,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct AlbumArtist {
        pub id: u64,
        pub name: String,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct SongArtist {
        pub id: u64,
        pub name: String,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct Privilege {
        pub id: u64,
        #[serde(rename = "plLevel")]
        pub pl_level: AudioQuality,
        #[serde(rename = "dlLevel")]
        pub dl_level: AudioQuality,
    }
}

pub mod song_detail {
    use crate::ncm::AudioQuality;
    use serde::Deserialize;

    /**
     * url: /song/detail?ids={},{...}
     */
    #[derive(Debug, Deserialize, Clone)]
    pub struct API {
        pub songs: Vec<SongDetail>,
        pub privileges: Vec<Privilege>,
        pub code: u16,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct SongDetail {
        pub id: u64,
        pub name: String,
        pub ar: Vec<SongArtist>,
        pub al: SongAlbum,
        pub no: u64,
        pub cd: String,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct SongArtist {
        pub id: u64,
        pub name: String,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct Privilege {
        pub id: u64,
        #[serde(rename = "plLevel")]
        pub pl_level: AudioQuality,
        #[serde(rename = "dlLevel")]
        pub dl_level: AudioQuality,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct SongAlbum {
        pub id: u64,
        pub name: String,
        #[serde(rename = "picUrl")]
        pub pic_url: String, // META attached_pic, may be different to cover.EXT
    }
}

pub mod song_url {
    use crate::ncm::AudioQuality;
    use serde::Deserialize;

    /**
     * url: /song/url/v1?id={},{...}&level={AudioQuality}
     */
    #[derive(Debug, Deserialize, Clone)]
    pub struct API {
        pub data: Vec<Payload>,
        pub code: u16,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct Payload {
        pub id: u64,
        pub url: String,
        pub level: AudioQuality,
        pub expi: u64,
        pub code: u16,
    }
}

pub mod lyric {
    use serde::Deserialize;

    /**
     * url: /lyric?id={}
     */
    #[derive(Debug, Deserialize, Clone)]
    pub struct API {
        pub lrc: Option<LrcPayload>,
        // 这个klyric逐词已经deprecated了
        // 之后可能考虑用/lyric/new
        // 但是那个api返回的格式并不是那么规范
        // pub klyric: KlyricPayload,
        pub tlyric: Option<TlyricPayload>,
        pub romalrc: Option<RomalrcPayload>,
        pub code: u16,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct LrcPayload {
        pub version: u32,
        pub lyric: String,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct TlyricPayload {
        pub version: u32,
        pub lyric: String,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct RomalrcPayload {
        pub version: u32,
        pub lyric: String,
    }
}
