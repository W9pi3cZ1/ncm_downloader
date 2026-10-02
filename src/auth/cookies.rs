use cookie::{Cookie, time};
use reqwest_cookie_store::CookieStore;
use std::fs;
use tracing::{error, warn};

/// 把 Netscape cookies.txt 的内容解析进 CookieStore
pub fn import_netscape(
    base_domain: &str,
    path: &str,
) -> Result<CookieStore, Box<dyn std::error::Error>> {
    let mut store = CookieStore::new();
    let content = fs::read_to_string(path)?;
    for (n, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }

        // 处理 #HttpOnly_ 前缀（Netscape 文件里常见）
        let (is_httponly, line) = if let Some(rest) = line.strip_prefix("#HttpOnly_") {
            (true, rest)
        } else if line.starts_with('#') {
            continue; // 普通注释行
        } else {
            (false, line)
        };

        // 7 个字段：domain  flag  path  secure  expires  name  value
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 7 {
            warn!("Invalid line in {}:{}", path, n);
            continue;
        }
        let (domain_o, path, secure_s, expires_s, name, value) =
            (parts[0], parts[2], parts[3], parts[4], parts[5], parts[6]);

        let mut c = Cookie::build((name, value));
        let domain = if domain_o.ends_with("163.com") {
            // rewrite to api domain
            base_domain
        } else {
            domain_o
        };
        if !domain.is_empty() {
            c = c.domain(domain)
        }
        if !path.is_empty() {
            c = c.path(path);
        }
        c = c
            .secure(secure_s.eq_ignore_ascii_case("TRUE"))
            .http_only(is_httponly);

        if expires_s != "0" {
            if let Ok(ts) = expires_s.parse::<i64>() {
                if let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(ts) {
                    c = c.expires(dt);
                }
            }
        }

        // URL 只需要用来确定默认 domain/path，用根域名即可
        let url_str = format!("https://{}", domain.trim_start_matches('.'));
        let raw = cookie_store::RawCookie::from(c.build());
        if let Ok(url) = url::Url::parse(&url_str) {
            // 解析失败就跳过这一条，不影响其他 cookie
            let _ = store.insert_raw(&raw, &url).inspect_err(|x| {
                error!("Parse Error: {:?}", x);
            });
        }
    }
    Ok(store)
}
