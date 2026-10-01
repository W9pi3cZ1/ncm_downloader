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
}
