mod ttml;

use std::{
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use cookie::time::{
    OffsetDateTime, UtcOffset,
    format_description::well_known::iso8601::FormattedComponents::DateTimeOffset,
};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use minijinja::{Environment, Error, ErrorKind};
use serde::Serialize;
use tokio::task::JoinSet;
use url::Url;

use crate::{
    model::{
        album::Album,
        downloader::{AlbumResources, Downloader},
        lyric::Lyrics,
        song::{Song, SongDisc},
    },
    tag,
};

/// 一次导出的产物。
#[derive(Debug, Default)]
pub struct AlbumExport {
    /// 专辑封面落盘路径（未提供 `cover_tmpl` 或封面资源缺失时为 None）
    pub cover: Option<PathBuf>,
    /// 每首歌：(song_id, 音频落盘路径)
    pub songs: Vec<(u64, PathBuf)>,
}

pub struct Exporter {
    pub downloader: Downloader,
    env: Environment<'static>,
}

impl Exporter {
    pub fn new(downloader: Downloader) -> Self {
        let mut env = Environment::new();
        env.set_trim_blocks(true);
        env.set_lstrip_blocks(true);
        env.set_keep_trailing_newline(false);
        env.add_filter("s", sanitize);
        env.add_filter("pad_to", |value: u32, total: u32| {
            let width = total.checked_ilog10().map_or(1, |d| d + 1) as usize;
            format!("{:0>width$}", value)
        });
        Self { downloader, env }
    }

    /* ==================== 下载 ==================== */

    /// 下载整张专辑的所有资源（音频 + 封面），带进度条。
    /// 临时目录随返回值 `AlbumResources` 一起管理，drop 时自动清理。
    pub async fn download_album(
        &self,
        album_id: u64,
    ) -> Result<AlbumResources, Box<dyn std::error::Error>> {
        let plan = self.downloader.plan_album_resources(album_id)?;
        let mp = MultiProgress::new();

        let bars: Arc<Vec<ProgressBar>> = Arc::new(
            plan.jobs
                .iter()
                .map(|job| {
                    let pb = mp.add(ProgressBar::new(0));
                    pb.set_style(
                        ProgressStyle::default_bar()
                            .template(
                                "{msg:25!} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
                            )
                            .unwrap()
                            .progress_chars("=>-"),
                    );
                    pb.set_message(job.label.clone());
                    pb
                })
                .collect(),
        );

        let overall = mp.add(ProgressBar::new(plan.jobs.len() as u64));
        overall.set_style(
            ProgressStyle::default_bar()
                .template("{msg:25!} [{bar:40.cyan/blue}] {pos}/{len}")
                .unwrap()
                .progress_chars("=>-"),
        );
        overall.set_message("all");

        let bars_for_factory = bars.clone();
        let overall_done = overall.clone();
        let resources = self
            .downloader
            .execute_album_resources(
                plan,
                move |i, _url, _path| {
                    let pb = bars_for_factory[i].clone();

                    let pb_chunk = pb.clone();
                    let on_chunk = move |recv: u64, total: Option<u64>| {
                        if let Some(t) = total {
                            if pb_chunk.length() != Some(t) {
                                pb_chunk.set_length(t);
                            }
                            if recv >= t && !pb_chunk.is_finished() {
                                pb_chunk.finish();
                            }
                        }
                        pb_chunk.set_position(recv);
                    };

                    let pb_retry = pb.clone();
                    let on_retry = move |attempt: usize, max: usize, _err: &str| {
                        pb_retry.reset();
                        pb_retry.set_message(format!("retry {attempt}/{max}"));
                    };

                    (on_chunk, on_retry)
                },
                move |n, _total| overall_done.set_position(n as u64),
            )
            .await?;

        overall.finish();
        Ok(resources)
    }

    /* ==================== 下载 + 渲染 + 落盘 ==================== */

    /// 下载专辑 → 用 `infer` 猜扩展名 → 渲染模板 → 复制到 `base_dir`。
    ///
    /// - `cover_tmpl`: `None` 时不导出封面；`Some` 时按专辑维度渲染一次
    /// - `song_tmpl` : 每首歌渲染一次
    pub async fn export_album(
        &self,
        album_id: u64,
        cover_tmpl: Option<&str>,
        song_tmpl: &str,
        base_dir: &Path,
    ) -> Result<AlbumExport, Box<dyn std::error::Error>> {
        let resources = Arc::new(Mutex::new(self.download_album(album_id).await?));

        // 抠出 song id，后续循环里可以重复借用 lib
        let sids: Vec<u64> = {
            let album = self
                .downloader
                .lib
                .get_album(album_id)
                .ok_or_else(|| format!("album {album_id} not found"))?;
            album.songs.iter().map(|s| s.0).collect()
        };

        let album = self
            .downloader
            .lib
            .get_album(album_id)
            .ok_or_else(|| format!("album {album_id} not found"))?;

        // —— 封面 ——
        let cover = match cover_tmpl {
            None => None,
            Some(tmpl) => {
                let pic_url = album.pic_url.clone().into_inner();
                // 只读操作：短暂持锁取出本地路径，之后马上释放
                let local = {
                    let res = resources.lock().unwrap();
                    res.get(&pic_url).map(|p| p.to_path_buf())
                };
                match local {
                    Some(local) => {
                        let ext = detect_extension(&local)?;
                        let dest = self.render_and_join(
                            tmpl,
                            &CoverPathCtx {
                                album: AlbumCtx::from(album),
                                ext: &ext,
                            },
                            base_dir,
                        )?;
                        // 写操作：再短暂持锁
                        resources.lock().unwrap().copy_to(&pic_url, &dest)?;
                        Some(dest)
                    }
                    None => None,
                }
            }
        };

        // -- 专辑共享数据 --
        let album_artists: Vec<String> = album.artists.iter().map(|a| a.name.clone()).collect();
        let release_local = OffsetDateTime::from(album.release_date)
            .to_offset(UtcOffset::from_hms(8, 0, 0).unwrap());
        let release_date: String = format!(
            "{:04}-{:02}-{:02}",
            release_local.year(),
            release_local.month() as u8,
            release_local.day()
        );
        // —— 逐首歌：准备 Job，然后丢进 blocking 线程池并行执行 ——
        let mut set: JoinSet<JobResult> = JoinSet::new();

        for (idx, sid) in album.songs.iter().map(|s| s.0).enumerate() {
            let Some(song) = self.downloader.lib.get_song(sid) else {
                continue;
            };
            let Some(audio_url) = song.url.as_ref().map(|u| u.url.clone().into_inner()) else {
                continue;
            };

            // 只读：拿本地路径 → 马上放锁
            let local = {
                let res = resources.lock().unwrap();
                res.get(&audio_url)
                    .ok_or_else(|| format!("audio missing for song {sid}"))?
                    .to_path_buf()
            };
            let ext = detect_extension(&local)?;

            let dest = self.render_and_join(
                song_tmpl,
                &SongPathCtx {
                    album: AlbumCtx::from(album),
                    song: SongCtx::from(song),
                    ext: &ext,
                },
                base_dir,
            )?;

            // 只读：读封面字节（本地临时文件）
            let cover_bytes = {
                let res = resources.lock().unwrap();
                res.get(&song.pic_url.clone().into_inner())
                    .and_then(|p| std::fs::read(p).ok())
            };

            let job = SongJob {
                idx,
                sid,
                audio_url,
                dest,
                title: song.name.clone(),
                artists: song.artists.iter().map(|a| a.name.clone()).collect(),
                track: song.discs.curr_track,
                track_total: song.discs.track_total,
                disc: song.discs.curr,
                disc_total: song.discs.total,
                disc_subtitle: song.discs.subtitle.clone(),
                lyrics: song.lyrics.clone(),
                cover_bytes,
                album_name: album.name.clone(),
                album_artists: album_artists.clone(),
                album_company: album.company.clone(),
                album_description: album.description.clone(),
                release_date: release_date.clone(),
            };

            let resources = Arc::clone(&resources);
            set.spawn_blocking(move || run_song_job(job, resources));
        }

        // —— 收集，按原始顺序返回 ——
        let mut ordered: Vec<(usize, u64, PathBuf)> = Vec::with_capacity(album.songs.len());
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(v)) => ordered.push(v),
                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(Box::new(e)),
            }
        }
        ordered.sort_by_key(|(i, _, _)| *i);
        let songs = ordered
            .into_iter()
            .map(|(_, sid, dest)| (sid, dest))
            .collect();

        Ok(AlbumExport { cover, songs })
    }

    /* ==================== 模板 ==================== */

    pub fn render_str<T: Serialize>(&self, tmpl: &str, ctx: &T) -> Result<String, Error> {
        self.env.template_from_str(tmpl)?.render(ctx)
    }

    /// 渲染 + 安全拼接，两处都用同一段逻辑。
    fn render_and_join<T: Serialize>(
        &self,
        tmpl: &str,
        ctx: &T,
        base_dir: &Path,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let rendered = self.render_str(tmpl, ctx)?;
        Ok(safe_join(base_dir, &rendered)?)
    }

    /// 专辑封面路径渲染（上下文只有 `album` + `ext`，没有 `song`）
    pub fn render_cover_path_with<'a>(
        &self,
        tmpl: &str,
        album: &'a Album,
        ext: &str,
    ) -> Result<PathBuf, Error> {
        let ctx = CoverPathCtx {
            album: AlbumCtx::from(album),
            ext,
        };
        let rendered = self.render_str(tmpl, &ctx)?;
        Ok(PathBuf::from(rendered))
    }

    pub fn render_cover_path(
        &self,
        tmpl: &str,
        album_id: u64,
        ext: &str,
    ) -> Result<PathBuf, Error> {
        let album = self.downloader.lib.get_album(album_id).ok_or_else(|| {
            Error::new(
                ErrorKind::UndefinedError,
                format!("album {album_id} not found"),
            )
        })?;
        self.render_cover_path_with(tmpl, album, ext)
    }

    pub fn render_song_path_with<'a>(
        &self,
        tmpl: &str,
        album: &'a Album,
        song: &'a Song,
        ext: &str,
    ) -> Result<PathBuf, Error> {
        let ctx = SongPathCtx {
            album: AlbumCtx::from(album),
            song: SongCtx::from(song),
            ext,
        };
        let rendered = self.render_str(tmpl, &ctx)?;
        Ok(PathBuf::from(rendered))
    }

    pub fn render_song_path(
        &self,
        tmpl: &str,
        album_id: u64,
        song_id: u64,
        ext: &str,
    ) -> Result<PathBuf, Error> {
        let album = self.downloader.lib.get_album(album_id).ok_or_else(|| {
            Error::new(
                ErrorKind::UndefinedError,
                format!("album {album_id} not found"),
            )
        })?;
        let song = self.downloader.lib.get_song(song_id).ok_or_else(|| {
            Error::new(
                ErrorKind::UndefinedError,
                format!("song {song_id} not found"),
            )
        })?;
        self.render_song_path_with(tmpl, album, song, ext)
    }
}

/* ==================== 单曲并行任务 ==================== */

fn write_lyrics_files(audio: &Path, lyrics: &Lyrics) -> std::io::Result<()> {
    let parent = audio.parent().unwrap_or_else(|| Path::new("."));
    let stem = audio.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-utf8 audio file stem")
    })?;

    let write_one = |suffix: &str, content: Option<&String>| -> std::io::Result<()> {
        if let Some(s) = content {
            if !s.is_empty() {
                std::fs::write(parent.join(format!("{stem}{suffix}.lrc")), s)?;
            }
        }
        Ok(())
    };

    write_one("", lyrics.orig.as_ref())?;
    write_one(".zh", lyrics.trans.as_ref())?;
    write_one(".romaji", lyrics.roma.as_ref())?;

    // 只要有任意一种语言，就写一份合并的 .ttml
    if lyrics.orig.is_some() || lyrics.trans.is_some() || lyrics.roma.is_some() {
        let ttml = lyrics.to_ttml();
        std::fs::write(parent.join(format!("{stem}.ttml")), ttml)?;
    }
    Ok(())
}

/// 每首歌在 spawn 前把需要的东西都搬到这里，方便闭包里直接 move。
struct SongJob {
    idx: usize,
    sid: u64,

    audio_url: Url,
    dest: PathBuf,

    // 曲目标签
    title: String,
    artists: Vec<String>,
    track: u32,
    track_total: u32,
    disc: u32,
    disc_total: u32,
    disc_subtitle: Option<String>,
    lyrics: Lyrics,
    cover_bytes: Option<Vec<u8>>,

    // 专辑级标签（每任务 clone 一份，避免借用问题）
    album_name: String,
    album_artists: Vec<String>,
    album_company: Option<String>,
    album_description: Option<String>,
    release_date: String,
}

// 任务返回值/错误类型，方便 JoinSet 标注
type JobResult = Result<(usize, u64, PathBuf), Box<dyn std::error::Error + Send + Sync>>;

/// 真正干活的：copy 音频 → 写标签。跑在 blocking 线程池里。
fn run_song_job(job: SongJob, resources: Arc<Mutex<AlbumResources>>) -> JobResult {
    // 复制音频到目标路径
    {
        let mut res = resources.lock().unwrap();
        res.copy_to(&job.audio_url, &job.dest)?;
    }

    // 先写歌词文件；失败只警告，不影响音频
    if let Err(e) = write_lyrics_files(&job.dest, &job.lyrics) {
        tracing::warn!("lyrics write failed for {}: {e}", job.dest.display());
    }

    // 封面 mime 在任务里现算，省得搬来搬去
    let cover_bytes = job.cover_bytes.as_deref();
    let cover_mime = cover_bytes.and_then(tag::image::detect);

    if let Err(e) = tag::writer::write_to(
        &job.dest,
        &tag::writer::TrackTags {
            title: &job.title,
            artists: &job.artists,
            album: &job.album_name,
            album_artists: &job.album_artists,
            track: job.track,
            track_total: job.track_total,
            disc: job.disc,
            disc_total: job.disc_total,
            disc_subtitle: job.disc_subtitle.as_deref(),
            release_date: &job.release_date,
            copyright: job.album_company.as_deref(),
            lyrics: job.lyrics.mix.as_deref(),
            description: job.album_description.as_deref(),
            cover: cover_bytes.zip(Some(cover_mime)),
        },
    ) {
        tracing::warn!("tag write failed for {}: {e}", job.dest.display());
    }

    Ok((job.idx, job.sid, job.dest))
}

/* ==================== 扩展名检测 ==================== */

/// 用 `infer` 从文件头判断真实格式。
/// 猜不出来时回退到原路径扩展名，再不行给 "bin"。
fn detect_extension(path: &Path) -> std::io::Result<String> {
    Ok(match infer::get_from_path(path)? {
        Some(t) => t.extension().to_string(),
        None => path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_string(),
    })
}

/* ==================== 路径安全拼接 ==================== */

/// 拒绝绝对路径 / `..` / 盘符，保证输出一定落在 `base` 内。
pub fn safe_join(base: &Path, rendered: &str) -> Result<PathBuf, std::io::Error> {
    let rel = Path::new(rendered);
    if rel.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("template produced an absolute path: {rendered}"),
        ));
    }
    for c in rel.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("template produced an unsafe path: {rendered}"),
                ));
            }
        }
    }
    Ok(base.join(rel))
}

/* ==================== 模板上下文 ==================== */

#[derive(Serialize)]
struct CoverPathCtx<'a> {
    album: AlbumCtx<'a>,
    ext: &'a str,
}

#[derive(Serialize)]
struct SongPathCtx<'a> {
    album: AlbumCtx<'a>,
    song: SongCtx<'a>,
    ext: &'a str,
}

#[derive(Serialize)]
struct AlbumCtx<'a> {
    id: u64,
    name: &'a str,
    artists: Vec<&'a str>,
    artist: String,
    pic_url: &'a str,
    release_year: u64,
    company: Option<&'a str>,
    description: Option<&'a str>,
    song_count: usize,
}

#[derive(Serialize)]
struct SongCtx<'a> {
    id: u64,
    name: &'a str,

    /// 碟号（`SongDisc::curr`）
    disc: u32,
    /// 本碟内曲目号（`SongDisc::curr_track`）
    track: u32,
    /// 本碟总轨数
    track_total: u32,
    /// 总碟数
    disc_total: u32,
    /// 碟副标题（可能没有）
    disc_subtitle: Option<&'a str>,

    artists: Vec<&'a str>,
    artist: String,
    pic_url: &'a str,
}

impl<'a> From<&'a Album> for AlbumCtx<'a> {
    fn from(a: &'a Album) -> Self {
        let artists: Vec<&str> = a.artists.iter().map(|x| x.name.as_str()).collect();
        let artist = artists.join("; ");
        let release_year = OffsetDateTime::from(a.release_date)
            .to_offset(UtcOffset::from_hms(8, 0, 0).unwrap())
            .year() as u64;
        Self {
            id: a.id.0,
            name: &a.name,
            artists,
            artist,
            pic_url: a.pic_url.as_str(),
            release_year,
            company: a.company.as_deref(),
            description: a.description.as_deref(),
            song_count: a.songs.len(),
        }
    }
}

impl<'a> From<&'a Song> for SongCtx<'a> {
    fn from(s: &'a Song) -> Self {
        let SongDisc {
            curr,
            curr_track,
            total,
            track_total,
            ref subtitle,
        } = s.discs;
        let artists: Vec<&str> = s.artists.iter().map(|x| x.name.as_str()).collect();
        let artist = artists.join("; ");
        Self {
            id: s.id.0,
            name: &s.name,
            disc: curr,
            track: curr_track,
            track_total,
            disc_total: total,
            disc_subtitle: subtitle.as_deref(),
            artists,
            artist,
            pic_url: s.pic_url.as_str(),
        }
    }
}

/* ==================== sanitize 过滤器 ==================== */

fn sanitize(input: String) -> String {
    let cleaned: String = input
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    cleaned.trim().trim_end_matches('.').to_string()
}
