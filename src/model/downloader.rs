use tracing::{info, warn};
use url::Url;

use crate::{
    model::{
        ArtistRef, Library, SongId,
        album::Album,
        song::{Lyrics, Song},
    },
    ncm::{AudioQuality, client::NCMClient},
};

/// Updater for library
pub struct Downloader {
    pub lib: Library,
    pub client: NCMClient,
}

impl Downloader {
    pub fn new(client: NCMClient) -> Self {
        Self {
            lib: Library::new(),
            client,
        }
    }
    pub async fn pull_album_info(&mut self, album_id: u64) {
        info!("Pulling album info for {}", album_id);
        let album_dto = self.client.album(album_id).await.unwrap();
        let album = Album::dto_album(album_dto.into_inner());
        let ids: Vec<_> = album.songs.iter().map(|x| x.0).collect();
        let songs_dto = self.client.song_detail(&ids).await.unwrap().into_inner();
        for s in songs_dto.songs {
            let song = Song {
                id: SongId(s.id),
                name: s.name.clone(),
                artists: s
                    .ar
                    .iter()
                    .map(|x| ArtistRef::new(x.id, x.name.clone()))
                    .collect(),
                album: album.id,
                pic_url: Url::parse(&s.al.pic_url).unwrap(),
                lyrics: Lyrics::empty(),
                discs: album.disc_map.find_song_disc(s.id).unwrap(),
                url: None,
            };
            self.lib.push_song(song);
        }
        self.lib.push_album(album);
    }
    pub async fn pull_album_play_urls(
        &mut self,
        album_id: u64,
        quality: AudioQuality,
    ) -> Option<()> {
        info!("Pulling album play urls for {}", album_id);
        let ids: Vec<_> = {
            let album = self.lib.get_album(album_id)?;
            album
                .songs
                .iter()
                .filter_map(|sid| self.lib.get_song(sid.0).map(|s| s.id.0))
                .collect()
        };

        let urls = self.client.song_url(&ids, quality).await.ok()?;
        for sid in ids {
            if let Some(song) = self.lib.get_song_mut(sid) {
                match urls.value.data.iter().find(|x| x.id == song.id.0) {
                    Some(x) => {
                        song.url = Some(urls.with_value_ref(x.clone()).into());
                    }
                    None => {
                        warn!("{:?} cannot be updated", song.id);
                    }
                }
            }
        }
        Some(())
    }
    pub async fn pull_album_lyrics(&mut self, album_id: u64) -> Option<()> {
        info!("Pulling album lyrics urls for {}", album_id);
        let ids: Vec<_> = {
            let album = self.lib.get_album(album_id)?;
            album
                .songs
                .iter()
                .filter_map(|sid| self.lib.get_song(sid.0).map(|s| s.id.0))
                .collect()
        };

        // let lyrics = self.client.lyrics(&ids).await.unwrap();
        let pb = indicatif::ProgressBar::new(ids.len() as u64);
        pb.set_style(
            indicatif::ProgressStyle::with_template(
                "{spinner:.green} {msg:25!} [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        pb.set_message("LYRICS");
        let pb_clone = pb.clone();
        let lyrics = self.client
            .lyrics_with_progress(&ids, move |n, _total| {
                pb_clone.set_position(n as u64)
            })
            .await.ok()?; 
        pb.finish_and_clear();
        for (nth, sid) in ids.iter().enumerate() {
            if let Some(song) = self.lib.get_song_mut(*sid) {
                song.lyrics = lyrics[nth].clone().into();
            }
        }
        Some(())
    }
}
