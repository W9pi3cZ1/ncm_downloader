use std::time::{Duration, SystemTime, UNIX_EPOCH};

use url::Url;

use crate::{
    model::{AlbumId, ArtistRef, SongId, song::SongDisc},
    ncm::dto,
    util::{get_disc_subtitle, non_empty},
};

#[derive(Debug)]
pub struct Disc {
    subtitle: Option<String>,
    raw: String,
    tracks: Vec<SongId>,
}

impl Disc {
    pub fn new(subtitle: String, raw: String) -> Self {
        Self {
            subtitle: non_empty(subtitle),
            raw,
            tracks: Vec::new(),
        }
    }

    pub fn add(&mut self, id: u64) {
        self.tracks.push(SongId(id));
    }
}

#[derive(Debug)]
pub struct DiscMap {
    discs: Vec<Disc>,
}

impl DiscMap {
    pub fn new() -> Self {
        Self { discs: Vec::new() }
    }

    pub fn push_track(&mut self, cd_raw: String, id: u64) {
        let disc = match self.discs.iter().position(|x| x.raw == cd_raw.clone()) {
            Some(d) => &mut self.discs[d],
            None => self
                .discs
                .push_mut(Disc::new(get_disc_subtitle(cd_raw.clone()), cd_raw.clone())),
        };
        disc.add(id);
    }

    pub fn find_song_disc(&self, id: u64) -> Option<SongDisc> {
        for (n, disc) in self.discs.iter().enumerate() {
            for (m, track) in disc.tracks.iter().enumerate() {
                if track.0 == id {
                    return Some(SongDisc {
                        curr: (n+1) as u32,
                        curr_track: (m+1) as u32,
                        total: self.discs.len() as u32,
                        subtitle: disc.subtitle.clone(),
                        track_total: disc.tracks.len() as u32,
                    })
                }
            }
        }
        None
    }
}

#[derive(Debug)]
pub struct Album {
    pub id: AlbumId,
    pub name: String,
    pub pic_url: Url,
    pub release_date: SystemTime,
    pub company: Option<String>,
    pub artists: Vec<ArtistRef>,
    pub comment: Option<String>,
    pub songs: Vec<SongId>,
    pub disc_map: DiscMap,
}

fn generate_disc_map(songs: Vec<dto::album::SongDetail>) -> DiscMap {
    let mut disc_map = DiscMap::new();
    songs.iter().for_each(|x| {
        disc_map.push_track(x.cd.clone(), x.id);
    });
    disc_map
}

impl Album {
    pub fn dto_album(value: dto::album::API) -> Self {
        let songs = value.songs;
        Self {
            id: AlbumId(value.album.id),
            name: value.album.name,
            pic_url: Url::parse(&value.album.pic_url).unwrap(),
            release_date: UNIX_EPOCH + Duration::from_millis(value.album.publish_time),
            company: non_empty(value.album.company),
            artists: value
                .album
                .artists
                .iter()
                .map(|x| ArtistRef::new(x.id, x.name.clone()))
                .collect(),
            comment: non_empty(value.album.description),
            songs: songs.iter().map(|x| SongId(x.id)).collect(),
            disc_map: generate_disc_map(songs),
        }
    }
}

// pub struct Missing;
// pub struct Finished;
// pub struct AlbumBuilder<S> {
//     id: AlbumId,
//     name: String,
//     pic_url: Option<Url>,
//     release_date: Option<SystemTime>,
//     company: Option<String>,
//     artists: Option<Vec<ArtistRef>>,
//     comment: Option<String>,
//     songs: Option<Vec<SongId>>,
//     _marker: PhantomData<S>,
// }

// impl AlbumBuilder<Missing> {
//     pub fn dto_album(self, value: dto::album::API) -> AlbumBuilder<Finished> {
//         AlbumBuilder {
//             id: self.id,
//             name: self.name,
//             pic_url: Some(Url::parse(&value.album.pic_url).unwrap()),
//             release_date: Some(UNIX_EPOCH + Duration::from_millis(value.album.publish_time)),
//             company: non_empty(value.album.company),
//             artists: Some(
//                 value
//                     .album
//                     .artists
//                     .iter()
//                     .map(|x| ArtistRef::new(x.id, x.name.clone()))
//                     .collect(),
//             ),
//             comment: non_empty(value.album.description),
//             songs: Some(value.songs.iter().map(|x| SongId(x.id)).collect()),
//             _marker: PhantomData,
//         }
//     }
// }

// impl AlbumBuilder<Finished> {
//     pub fn build(self) -> Album {
//         Album {
//             id: self.id,
//             name: self.name,
//             pic_url: self.pic_url.unwrap(),
//             release_date: self.release_date.unwrap(),
//             company: self.company,
//             artists: self.artists.unwrap(),
//             comment: self.comment,
//             songs: self.songs.unwrap(),
//         }
//     }
// }
