use crate::{
    auth::cookies::import_netscape,
    ncm::{AudioQuality, DEFAULT_NCMEAPI_URL, dto},
};
use reqwest::Client;
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use rustls::ClientConfig;
use serde::de::DeserializeOwned;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};
use tracing::{debug, error, info};
use url::Url;

#[cfg(not(target_os = "android"))]
pub fn build_tls_config() -> ClientConfig {
    // 桌面 / iOS：直接用平台验证器
    use rustls_platform_verifier::ConfigVerifierExt;
    ClientConfig::with_platform_verifier().expect("Failed to create platform verifier")
}

#[cfg(target_os = "android")]
pub fn build_tls_config() -> ClientConfig {
    use rustls_platform_verifier::ConfigVerifierExt;
    use tracing::warn;
    if std::env::var("TERMUX_VERSION").is_ok() {
        // Termux 回退
        let mut roots = rustls::RootCertStore::empty();

        // 0.8+ 直接返回 CertificateResult，不再包 Result
        let result = rustls_native_certs::load_native_certs();

        // 忽略错误，但可以在日志里记一下（errors 可能非空但 certs 也有内容）
        for err in &result.errors {
            warn!("Failed to load native cert: {err}");
        }

        for cert in result.certs {
            let _ = roots.add(cert); // 忽略重复证书
        }

        // 如果系统证书一个都没加载到，回退到 webpki-roots
        if roots.is_empty() {
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        }

        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    } else {
        // 普通 Android App
        ClientConfig::with_platform_verifier().expect("Failed to create platform verifier")
    }
}

/// 一次请求的时序信息
///
/// - `sent_at` / `received_at` 是墙钟（`SystemTime`），用于计算「链接何时过期」这类绝对时刻
/// - `elapsed` 由单调时钟（`Instant`）测得，不受用户调整系统时间影响
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// 请求「发出」时的墙钟时刻
    pub sent_at: SystemTime,
    /// 「完整收到响应体」时的墙钟时刻
    pub received_at: SystemTime,
    /// 从发出到收完 body 的耗时
    pub elapsed: Duration,
}

/// 携带时序信息的响应体
#[derive(Debug, Clone)]
pub struct Timed<T> {
    pub value: T,
    pub timing: Timing,
}

impl<T> Timed<T> {
    pub fn new(value: T, timing: Timing) -> Self {
        Self { value, timing }
    }

    /// 取出时序信息（`Timing: Copy`，不消耗 self）
    pub fn timing(&self) -> Timing {
        self.timing
    }

    /// 转换内部值，保留时序信息
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Timed<U> {
        Timed {
            value: f(self.value),
            timing: self.timing,
        }
    }

    /// 用新值替换内部值，保留时序信息（消耗 self）
    pub fn with_value<U>(self, value: U) -> Timed<U> {
        Timed {
            value,
            timing: self.timing,
        }
    }

    /// 借用旧的时序信息，配一个新值；旧 `Timed` 仍然可用
    pub fn with_value_ref<U>(&self, value: U) -> Timed<U> {
        Timed {
            value,
            timing: self.timing,
        }
    }

    /// 取出内部值，丢弃时序信息
    pub fn into_inner(self) -> T {
        self.value
    }

    /// 解构成 (值, 时序信息)
    pub fn into_parts(self) -> (T, Timing) {
        (self.value, self.timing)
    }
}

/// 单个下载任务的总结
#[derive(Debug, Clone, Copy)]
pub struct DownloadResult {
    /// 实际写入的字节数
    pub bytes: u64,
    /// 服务端声明的 Content-Length（如果有）
    pub content_length: Option<u64>,
}

/// `download_*` 内部错误：区分「可重试」和「不可重试」。
///
/// 4xx 通常意味着签名过期、权限不足，重试没意义；
/// 5xx / 网络抖动 / IO 错误才值得退避重试。
#[derive(Debug)]
enum DlErr {
    Fatal(String),
    Retry(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for DlErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DlErr::Fatal(s) => write!(f, "{}", s),
            DlErr::Retry(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for DlErr {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DlErr::Fatal(_) => None,
            DlErr::Retry(e) => Some(e.as_ref()),
        }
    }
}

impl From<reqwest::Error> for DlErr {
    fn from(e: reqwest::Error) -> Self {
        DlErr::Retry(Box::new(e))
    }
}

impl From<std::io::Error> for DlErr {
    fn from(e: std::io::Error) -> Self {
        DlErr::Retry(Box::new(e))
    }
}

#[derive(Debug)]
pub struct NCMClient {
    pub client: Client,
    pub attempts: usize,
    pub concurrent: usize,
    pub base_url: Url,
}

impl NCMClient {
    pub fn new(
        cookies_path: Option<String>,
        attempts: usize,
        concurrent: usize,
        base_url: String,
    ) -> Self {
        let base_url = match Url::parse(&base_url) {
            Ok(x) => x,
            Err(_) => Url::parse(DEFAULT_NCMEAPI_URL).unwrap(),
        };
        let cookie_store = if let Some(path) = cookies_path {
            match import_netscape(base_url.host_str().unwrap(), &path) {
                Ok(x) => {
                    info!("Load cookies success");
                    x
                }
                Err(e) => {
                    error!("Failed to load cookie: {:?}", e);
                    info!("No cookies were imported");
                    CookieStore::new()
                }
            }
        } else {
            info!("No cookies were imported");
            CookieStore::new()
        };

        Self {
            client: Client::builder()
                .use_preconfigured_tls(build_tls_config())
                .redirect(reqwest::redirect::Policy::limited(10))
                .read_timeout(std::time::Duration::from_secs(30))
                .timeout(std::time::Duration::from_secs(86400))
                .cookie_provider(Arc::new(CookieStoreMutex::new(cookie_store)))
                .build()
                .unwrap(),
            attempts,
            concurrent,
            base_url,
        }
    }

    pub fn pathname(url: &str) -> String {
        // Remove scheme://
        let (had_scheme, s) = match url.split_once("://") {
            Some((_, rest)) => (true, rest),
            None => (false, url),
        };

        // Remove host
        let s: &str = if had_scheme {
            s.find('/').map(|i| &s[i..]).unwrap_or("/")
        } else if let Some(i) = s.find('/') {
            let head = &s[..i];
            if !head.is_empty() && head.contains('.') {
                &s[i..]
            } else {
                s
            }
        } else {
            s
        };

        // Use fragment if s has '#'
        let s = s.split_once('#').map(|(_, f)| f).unwrap_or(s);

        // Remove '/' and 'm/' prefix
        let s = s.trim_start_matches('/');
        let s = s.strip_prefix("m/").unwrap_or(s);

        // Get path / query
        let (path, query) = match s.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (s, None),
        };

        // Get id property from query
        let id = query.and_then(|q| {
            q.split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| *k == "id")
                .map(|(_, v)| v)
        });

        // Splice together
        match id {
            Some(v) => format!("/{}/{}", path.trim_end_matches('/'), v),
            None => format!("/{}", path.trim_end_matches("/")),
        }
    }

    pub fn extract_album_id(url: &str) -> Option<u64> {
        let pathname = Self::pathname(url);
        info!("Pathname: {}", pathname);
        let id_str = match pathname.split_once("/album/") {
            Some((_, rest)) => rest,
            None => return None,
        };
        match id_str.parse::<u64>() {
            Ok(id) => Some(id),
            Err(_) => None,
        }
    }

    pub async fn get_album_id(&self, url: &str) -> Option<u64> {
        // if short link passed
        let binding;
        let url = if !url.contains("music.163.com") {
            match self.resolve_short_url(url).await {
                Some(real_url) => {
                    binding = real_url.to_string();
                    println!("EXPANDED -> {}", binding);
                    &binding
                }
                None => url,
            }
        } else {
            url
        };
        Self::extract_album_id(url)
    }

    pub async fn resolve_short_url(&self, i: &str) -> Option<Url> {
        Some(self.client.clone().get(i).send().await.ok()?.url().clone())
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Timed<T>, Box<dyn std::error::Error>> {
        let url = self.base_url.join(path)?;
        let attempts = self.attempts.max(1);
        let mut last_err: Box<dyn std::error::Error> = "request failed".into();

        for attempt in 1..=attempts {
            debug!("Sent (attempt {}/{}): {}", attempt, attempts, path);

            // 记录时间
            let sent_at = SystemTime::now();
            let start = Instant::now();

            // 1) 发送请求
            let resp = match self.client.get(url.clone()).send().await {
                Ok(r) => r,
                Err(e) => {
                    error!("Attempt {}/{} send error: {}", attempt, attempts, e);
                    last_err = Box::new(e);
                    Self::backoff(attempt, attempts).await;
                    continue;
                }
            };

            let status = resp.status();
            debug!("Recv: {} from {}", status, path);

            // 2) 5xx 视为可重试
            if status.is_server_error() {
                let msg = format!("server error: {} for {}", status, path);
                error!("Attempt {}/{}: {}", attempt, attempts, msg);
                last_err = msg.into();
                Self::backoff(attempt, attempts).await;
                continue;
            }

            // 3) 其他非 2xx 直接返回（例如 401 / 403 重试没意义）
            if !status.is_success() {
                return Err(format!("HTTP {} for {}", status, path).into());
            }

            // 4) 反序列化
            match resp.json::<T>().await {
                Ok(v) => {
                    let received_at = SystemTime::now();
                    let elapsed = start.elapsed();
                    return Ok(Timed {
                        value: v,
                        timing: Timing {
                            sent_at,
                            received_at,
                            elapsed,
                        },
                    });
                }
                Err(e) => {
                    error!("Attempt {}/{} parse error: {}", attempt, attempts, e);
                    last_err = Box::new(e);
                    Self::backoff(attempt, attempts).await;
                }
            }
        }

        Err(last_err)
    }

    /// 指数退避，最后一次不 sleep
    async fn backoff(attempt: usize, attempts: usize) {
        if attempt >= attempts {
            return;
        }
        let shift = (attempt - 1).min(5) as u32; // 最多 2^5
        let ms = 200u64.saturating_mul(1u64 << shift); // 200, 400, 800, ...
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    fn ids_string(ids: &[u64]) -> String {
        ids.iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub async fn album(
        &self,
        id: u64,
    ) -> Result<Timed<dto::album::API>, Box<dyn std::error::Error>> {
        self.get_json(&format!("/album?id={}", id)).await
    }

    pub async fn song_detail(
        &self,
        ids: &[u64],
    ) -> Result<Timed<dto::song_detail::API>, Box<dyn std::error::Error>> {
        self.get_json(&format!("/song/detail?ids={}", Self::ids_string(ids)))
            .await
    }

    pub async fn song_url(
        &self,
        ids: &[u64],
        quality: AudioQuality,
    ) -> Result<Timed<dto::song_url::API>, Box<dyn std::error::Error>> {
        self.get_json(&format!(
            "/song/url?id={}&level={}",
            Self::ids_string(ids),
            quality
        ))
        .await
    }

    pub async fn lyric(
        &self,
        id: u64,
    ) -> Result<Timed<dto::lyric::API>, Box<dyn std::error::Error>> {
        self.get_json(&format!("/lyric?id={}", id)).await
    }

    pub async fn lyrics(
        &self,
        ids: &[u64],
    ) -> Result<Vec<Timed<dto::lyric::API>>, Box<dyn std::error::Error>> {
        use futures::stream::{self, StreamExt};

        let concurrent = self.concurrent.max(1);

        // 预先准备定长槽位
        let mut slots: Vec<Option<Timed<dto::lyric::API>>> = (0..ids.len()).map(|_| None).collect();

        // (i, id) 一起进 future，返回时带回 i
        let mut s = stream::iter(ids.iter().copied().enumerate())
            .map(|(i, id)| async move { (i, self.lyric(id).await) })
            .buffer_unordered(concurrent);

        while let Some((i, r)) = s.next().await {
            slots[i] = Some(r?); // 出错直接短路
        }

        Ok(slots.into_iter().map(Option::unwrap).collect())
    }

    pub async fn lyrics_with_progress<F>(
        &self,
        ids: &[u64],
        on_done: F,
    ) -> Result<Vec<Timed<dto::lyric::API>>, Box<dyn std::error::Error>>
    where
        F: Fn(usize, usize) + Send + Sync + 'static,
    {
        use futures::stream::{self, StreamExt};
        let total = ids.len();
        let done = Arc::new(AtomicUsize::new(0));
        let on_done = Arc::new(on_done);

        let concurrent = self.concurrent.max(1);

        // 预先准备定长槽位
        let mut slots: Vec<Option<Timed<dto::lyric::API>>> = (0..total).map(|_| None).collect();

        // (i, id) 一起进 future，返回时带回 i
        let mut s = stream::iter(ids.iter().copied().enumerate())
            .map(|(i, id)| {
                let done = done.clone();
                let on_done = on_done.clone();
                async move {
                    let r = (i, self.lyric(id).await);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    on_done(n, total);
                    r
                }
            })
            .buffer_unordered(concurrent);

        while let Some((i, r)) = s.next().await {
            slots[i] = Some(r?); // 出错直接短路
        }

        Ok(slots.into_iter().map(Option::unwrap).collect())
    }

    // ==== 下载 ====

    /// 单 URL 流式下载，只发一次请求。
    ///
    /// 4xx 返回 `Fatal`；5xx / 网络 / IO 返回 `Retry`。
    async fn download_once<W, F>(
        &self,
        url: &str,
        writer: &mut W,
        on_chunk: &F,
    ) -> Result<Timed<DownloadResult>, DlErr>
    where
        W: tokio::io::AsyncWrite + Unpin + ?Sized,
        F: Fn(u64, Option<u64>) + Send + Sync,
    {
        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;

        debug!("Download: {}", url);
        let sent_at = SystemTime::now();
        let start = Instant::now();

        let resp = self.client.get(url).send().await?;
        let status = resp.status();
        if status.is_server_error() {
            return Err(DlErr::Retry(format!("server error: {}", status).into()));
        }
        if !status.is_success() {
            return Err(DlErr::Fatal(format!("HTTP {} for {}", status, url)));
        }

        // 可能为 None（chunked transfer 没有 Content-Length）
        let content_length = resp.content_length();

        // 通知一开始的状态（position=0，length 可能已知可能未知）
        on_chunk(0, content_length);

        let mut stream = resp.bytes_stream();
        let mut bytes: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            writer.write_all(&chunk).await?;
            bytes += chunk.len() as u64;
            on_chunk(bytes, content_length); // 每个 chunk
        }
        writer.flush().await?;

        let received_at = SystemTime::now();
        let elapsed = start.elapsed();
        Ok(Timed {
            value: DownloadResult {
                bytes,
                content_length,
            },
            timing: Timing {
                sent_at,
                received_at,
                elapsed,
            },
        })
    }

    pub async fn download_to_path_with<F, R>(
        &self,
        url: &str,
        path: impl AsRef<Path>,
        on_chunk: F,
        on_retry: R,
    ) -> Result<Timed<DownloadResult>, Box<dyn std::error::Error>>
    where
        F: Fn(u64, Option<u64>) + Send + Sync,
        R: Fn(usize, usize, &str) + Send + Sync,
    {
        let path = path.as_ref();
        let attempts = self.attempts.max(1);
        let mut last_err: Box<dyn std::error::Error> = "download failed".into();

        for attempt in 1..=attempts {
            // 每次重试都 truncate，避免半截文件叠加
            let mut file = match tokio::fs::File::create(path).await {
                Ok(f) => f,
                Err(e) => return Err(Box::new(e)),
            };

            match self.download_once(url, &mut file, &on_chunk).await {
                Ok(v) => return Ok(v),
                Err(DlErr::Fatal(s)) => return Err(s.into()),
                Err(DlErr::Retry(e)) => {
                    error!(
                        "Download attempt {}/{} failed: {} ({})",
                        attempt, attempts, e, url
                    );
                    last_err = e;
                    if attempt < attempts {
                        on_retry(attempt, attempts, &last_err.to_string());
                        Self::backoff(attempt, attempts).await;
                    }
                }
            }
        }

        Err(last_err)
    }

    /// 下载到文件，自动重试。
    ///
    /// - 5xx / 网络 / IO 错误会按 [`Self::backoff`] 退避重试
    /// - 每次重试前 **truncate 目标文件**，确保不会留下半截内容
    /// - 4xx（如签名过期）直接返回，不重试
    pub async fn download_to_path(
        &self,
        url: &str,
        path: impl AsRef<Path>,
    ) -> Result<Timed<DownloadResult>, Box<dyn std::error::Error>> {
        self.download_to_path_with(url, path, |_, _| {}, |_, _, _| {})
            .await
    }

    /// 批量下载 + 每 job 独立的 chunk/retry 回调。
    ///
    /// - `make_callbacks(i, url, path)`: 为第 i 个任务生成专属的
    ///   `(on_chunk, on_retry)`。典型用法是在这里取出第 i 根 `ProgressBar`
    ///   并 clone 进闭包。
    /// - `on_done(n, total)`: 每个 job 完成（含失败）后调用一次。
    /// - 任意 job 彻底失败会短路，返回该错误。
    pub async fn download_to_paths_with<CF, RF, MF, D>(
        &self,
        jobs: Vec<(String, PathBuf)>,
        make_callbacks: MF,
        on_done: D,
    ) -> Result<Vec<Timed<DownloadResult>>, Box<dyn std::error::Error>>
    where
        CF: Fn(u64, Option<u64>) + Send + Sync + 'static,
        RF: Fn(usize, usize, &str) + Send + Sync + 'static,
        MF: Fn(usize, &str, &Path) -> (CF, RF) + Send + Sync + 'static,
        D: Fn(usize, usize) + Send + Sync + 'static,
    {
        use futures::stream::{self, StreamExt};

        let total = jobs.len();
        let done = Arc::new(AtomicUsize::new(0));
        let on_done = Arc::new(on_done);
        let make_callbacks = Arc::new(make_callbacks);
        let concurrent = self.concurrent.max(1);

        let mut slots: Vec<Option<Timed<DownloadResult>>> = (0..total).map(|_| None).collect();

        let mut s = stream::iter(jobs.into_iter().enumerate())
            .map(|(i, (url, path))| {
                let done = done.clone();
                let on_done = on_done.clone();
                let make_callbacks = make_callbacks.clone();
                async move {
                    // 每 job 生成专属回调
                    let (on_chunk, on_retry) = make_callbacks(i, &url, &path);
                    let r = self
                        .download_to_path_with(&url, &path, on_chunk, on_retry)
                        .await;

                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    on_done(n, total);
                    (i, r)
                }
            })
            .buffer_unordered(concurrent);

        while let Some((i, r)) = s.next().await {
            slots[i] = Some(r?);
        }

        Ok(slots.into_iter().map(Option::unwrap).collect())
    }

    /// 批量下载：并发把每个 `(url, 目标路径)` 下载到文件。
    ///
    /// - 并发数由 `self.concurrent` 控制
    /// - `on_done(已完成数, 总数)` 在**每个 job 完成时**被调用（含失败的）
    /// - 任意一个 job 彻底失败（重试用尽 / 4xx）会短路整个批量，返回该错误
    ///
    /// 需要「部分失败不影响其余」请自行循环调用 [`Self::download_to_path`]。
    pub async fn download_to_paths<D>(
        &self,
        jobs: Vec<(String, PathBuf)>,
        on_done: D,
    ) -> Result<Vec<Timed<DownloadResult>>, Box<dyn std::error::Error>>
    where
        D: Fn(usize, usize) + Send + Sync + 'static,
    {
        self.download_to_paths_with(
            jobs,
            |_, _, _| {
                (
                    |_: u64, _: Option<u64>| {},
                    |_: usize, _: usize, _: &str| {},
                )
            },
            on_done,
        )
        .await
    }
}
