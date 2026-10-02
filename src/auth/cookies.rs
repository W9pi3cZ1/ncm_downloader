use cookie::time;
use reqwest_cookie_store::CookieStore;
use std::fs;
use tracing::warn;

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
        let (domain, path, secure_s, expires_s, name, value) =
            (parts[0], parts[2], parts[3], parts[4], parts[5], parts[6]);

        // 拼成 Set-Cookie 字符串
        // 不要担心什么性能问题，服务器会设的cookie不会太多的...
        let mut s = format!("{name}={value}");
        if !domain.is_empty() {
            if domain.ends_with("163.com") {
                // rewrite to api domain
                s.push_str("; Domain=");
                s.push_str(base_domain);
            } else {
                s.push_str("; Domain=");
                s.push_str(domain);
            }
        }
        if !path.is_empty() {
            s.push_str("; Path=");
            s.push_str(path);
        }
        if secure_s.eq_ignore_ascii_case("TRUE") {
            s.push_str("; Secure");
        }
        if is_httponly {
            s.push_str("; HttpOnly");
        }
        if expires_s != "0" {
            if let Ok(ts) = expires_s.parse::<i64>() {
                if let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(ts) {
                    if let Ok(fmt) = dt.format(&time::format_description::well_known::Rfc2822) {
                        s.push_str("; Expires=");
                        s.push_str(&fmt);
                    }
                }
            }
        }

        // URL 只需要用来确定默认 domain/path，用根域名即可
        let url_str = format!("https://{}", domain.trim_start_matches('.'));
        if let Ok(url) = url::Url::parse(&url_str) {
            // 解析失败就跳过这一条，不影响其他 cookie
            let _ = store.insert_raw(&cookie_store::RawCookie::from(s), &url);
        }
    }
    Ok(store)
}
