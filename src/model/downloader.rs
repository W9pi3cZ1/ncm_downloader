use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use tempfile::TempDir;
use tracing::{info, warn};
use url::Url;

use crate::{
    model::{ArtistRef, Library, SongId, album::Album, lyric::Lyrics, song::Song},
    ncm::{AudioQuality, client::NCMClient},
    util::encode_base36,
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
                pic_url: Url::parse(&s.al.pic_url).unwrap().into(),
                lyrics: Lyrics::new(),
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
        let lyrics = self
            .client
            .lyrics_with_progress(&ids, move |n, _total| pb_clone.set_position(n as u64))
            .await
            .ok()?;
        pb.finish_and_clear();
        for (nth, sid) in ids.iter().enumerate() {
            if let Some(song) = self.lib.get_song_mut(*sid) {
                song.lyrics = lyrics[nth].clone().into();
            }
        }
        Some(())
    }

    /// 纯规划，不发网络请求。
    ///
    /// - 遍历专辑 + 所有歌曲，按 `url.path()` 去重
    /// - 顺序：先专辑封面，再按歌曲顺序逐个音频/封面
    pub fn plan_album_resources(
        &self,
        album_id: u64,
    ) -> Result<AlbumResourcePlan, Box<dyn std::error::Error>> {
        let album = self
            .lib
            .get_album(album_id)
            .ok_or_else(|| format!("album {album_id} not found"))?;

        // 有序 + 去重：Vec 保插入顺序，HashSet 判重
        let mut seen: HashSet<String> = HashSet::new();
        let mut ordered: Vec<(String, Url, String)> = Vec::new();

        let push = |url: Url, label: String, seen: &mut HashSet<String>, out: &mut Vec<_>| {
            let key = url.path().to_owned();
            if seen.insert(key.clone()) {
                out.push((key, url, label));
            }
        };

        let album_pic: Url = album.pic_url.clone().into_inner();
        push(
            album_pic,
            format!("cover · {}", album.name),
            &mut seen,
            &mut ordered,
        );

        for (nth, sid) in album.songs.iter().enumerate() {
            let song = self
                .lib
                .get_song(sid.0)
                .ok_or_else(|| format!("song {sid:?} not found"))?;

            let pic: Url = song.pic_url.clone().into_inner();
            push(
                pic,
                format!("cover · {}", song.name),
                &mut seen,
                &mut ordered,
            );

            if let Some(u) = &song.url {
                let audio: Url = u.url.clone().into_inner();
                push(
                    audio,
                    format!("{:05} · {}", nth, song.name,),
                    &mut seen,
                    &mut ordered,
                );
            }
        }

        let tmp = tempfile::Builder::new().prefix("ncmd-").tempdir()?;
        let jobs = ordered
            .into_iter()
            .enumerate()
            .map(|(nth, (path_key, url, label))| ResourceJob {
                url,
                path_key,
                local_path: tmp.path().join(encode_base36(nth)),
                label,
            })
            .collect();

        Ok(AlbumResourcePlan { jobs, tmp })
    }

    /// 执行下载计划。签名与 `NCMClient::download_to_paths_with` 一致，
    /// 让上层自己决定每个 job 挂什么进度条。
    pub async fn execute_album_resources<CF, RF, MF, D>(
        &self,
        plan: AlbumResourcePlan,
        make_callbacks: MF,
        on_done: D,
    ) -> Result<AlbumResources, Box<dyn std::error::Error>>
    where
        CF: Fn(u64, Option<u64>) + Send + Sync + 'static,
        RF: Fn(usize, usize, &str) + Send + Sync + 'static,
        MF: Fn(usize, &str, &Path) -> (CF, RF) + Send + Sync + 'static,
        D: Fn(usize, usize) + Send + Sync + 'static,
    {
        let AlbumResourcePlan { jobs, tmp } = plan;

        // path_key -> local_path 先备份好，等下载完再装进 AlbumResources
        let files: HashMap<String, PathBuf> = jobs
            .iter()
            .map(|j| (j.path_key.clone(), j.local_path.clone()))
            .collect();

        let dl_jobs: Vec<(String, PathBuf)> = jobs
            .iter()
            .map(|j| (j.url.as_str().to_owned(), j.local_path.clone()))
            .collect();

        self.client
            .download_to_paths_with(dl_jobs, make_callbacks, on_done)
            .await?;

        Ok(AlbumResources { tmp, files })
    }
}

/// 专辑相关资源已经全部落到临时目录。
/// 生命周期结束时目录自动删除。
#[derive(Debug)]
pub struct AlbumResources {
    /// 保活：`TempDir` drop 时整个目录被删掉
    tmp: TempDir,
    /// url.path() -> 本地文件路径
    files: HashMap<String, PathBuf>,
}

#[allow(unused)]
impl AlbumResources {
    /// 根据完整 URL 查询（自动忽略 host/query，只看 path）
    pub fn get(&self, url: &Url) -> Option<&Path> {
        self.get_by_path(url.path())
    }

    /// 直接根据 path 查询
    pub fn get_by_path(&self, path: &str) -> Option<&Path> {
        self.files.get(path).map(PathBuf::as_path)
    }

    /// 所有资源都放在这下面
    pub fn root(&self) -> &Path {
        self.tmp.path()
    }

    /// 取出（移动到目标位置）。适合渲染完模板、写出最终文件时调用。
    pub fn copy_to(&mut self, url: &Url, dest: impl AsRef<Path>) -> std::io::Result<PathBuf> {
        let src = self
            .files
            .get(url.path())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "resource not found"))?
            .clone();
        let dest = dest.as_ref();
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(&src, dest)?;
        Ok(dest.to_path_buf())
    }

    pub fn keep(self) -> PathBuf {
        self.tmp.keep()
    }
}

/// 规划好的专辑资源下载计划：一次遍历 Library 得到所有需要下载的 URL。
/// 携带 `TempDir`，drop 时连同临时文件一起清理。
pub struct AlbumResourcePlan {
    pub jobs: Vec<ResourceJob>,
    tmp: TempDir,
}

/// 单个待下载资源。
#[derive(Debug, Clone)]
pub struct ResourceJob {
    /// 下载地址
    pub url: Url,
    /// `url.path()`，同时作为 `AlbumResources` 的查询 key
    pub path_key: String,
    /// 临时目录里的落点
    pub local_path: PathBuf,
    /// 给人看的标签（歌名 / 封面名），用来 set_message
    pub label: String,
}
