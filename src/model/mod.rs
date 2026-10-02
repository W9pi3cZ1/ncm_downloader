pub mod downloader;
pub use downloader::Downloader;

use std::collections::HashMap;

use crate::model::{album::Album, song::Song};

pub mod album;
pub mod lyric;
pub mod song;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u64);
        impl From<u64> for $name {
            fn from(v: u64) -> Self {
                Self(v)
            }
        }
        impl From<$name> for u64 {
            fn from(v: $name) -> u64 {
                v.0
            }
        }
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }
    };
}
id_type!(AlbumId);
id_type!(ArtistId);
id_type!(SongId);

#[derive(Debug, Clone)]
pub struct ArtistRef {
    pub id: Option<ArtistId>, // 可能出现未绑上的情况
    pub(crate) name: String,
}

impl ArtistRef {
    pub fn new(n: u64, s: String) -> Self {
        if n == 0 {
            Self { id: None, name: s }
        } else {
            Self {
                id: Some(ArtistId(n)),
                name: s,
            }
        }
    }
}

pub struct Library {
    pub album_pools: HashMap<AlbumId, Album>,
    pub song_pools: HashMap<SongId, Song>,
}

impl Library {
    pub fn new() -> Self {
        Self {
            album_pools: HashMap::new(),
            song_pools: HashMap::new(),
        }
    }

    pub fn push_song(&mut self, song: Song) {
        self.song_pools.insert(song.id, song);
    }

    pub fn push_album(&mut self, album: Album) {
        self.album_pools.insert(album.id, album);
    }

    pub fn get_album(&self, id: u64) -> Option<&Album> {
        self.album_pools.get(&AlbumId(id))
    }

    pub fn get_album_mut(&mut self, id: u64) -> Option<&mut Album> {
        self.album_pools.get_mut(&AlbumId(id))
    }

    pub fn get_song(&self, id: u64) -> Option<&Song> {
        self.song_pools.get(&SongId(id))
    }

    pub fn get_song_mut(&mut self, id: u64) -> Option<&mut Song> {
        self.song_pools.get_mut(&SongId(id))
    }
}
