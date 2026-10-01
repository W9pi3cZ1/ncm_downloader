mod downloader;
pub use downloader::Downloader;

use std::collections::HashMap;

use url::Url;

use crate::{
    model::{album::Album, song::{Lyrics, Song}}, ncm::client::NCMClient,
};

mod album;
mod song;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct AlbumId(u64);
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ArtistId(u64);
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct SongId(u64);

#[derive(Debug, Clone)]
pub struct ArtistRef {
    id: Option<ArtistId>, // 可能出现未绑上的情况
    name: String,
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

    pub fn push_song(&mut self, song: Song){
        self.song_pools.insert(song.id, song);
    }

    pub fn push_album(&mut self, album: Album){
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
