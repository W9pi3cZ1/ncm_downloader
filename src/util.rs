pub fn non_empty<T: AsRef<str>>(s: T) -> Option<T> {
    (!s.as_ref().is_empty()).then_some(s)
}

pub fn get_disc_subtitle(disc_raw: String) -> String {
    let mut disc_name = disc_raw.as_str();
    if disc_name.starts_with(|c: char| c.is_ascii_digit()) {
        disc_name = disc_name.trim_start_matches(|c: char| c.is_ascii_digit());
        if let Some(rest) = disc_name.strip_prefix('/') {
            let after_slash_digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            if after_slash_digits.len() != rest.len() {
                disc_name = after_slash_digits;
            }
        }
    }
    disc_name.trim().to_owned()
}

// util.rs
use std::fmt;
use std::ops::Deref;
use url::Url;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PrettyUrl(Url);

impl PrettyUrl {
    pub fn new(url: Url) -> Self {
        Self(url)
    }
    pub fn into_inner(self) -> Url {
        self.0
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Url> for PrettyUrl {
    fn from(u: Url) -> Self {
        Self(u)
    }
}

impl Deref for PrettyUrl {
    type Target = Url;
    fn deref(&self) -> &Url {
        &self.0
    }
}

impl fmt::Debug for PrettyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 想更像原 Debug 就写 Url("...")，想极简直接写 "..." 也行
        write!(f, "Url({})", self.0)
    }
}

impl fmt::Display for PrettyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub fn encode_base36(mut n: usize) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::with_capacity(13);
    while n > 0 {
        buf.push(DIGITS[n % 36]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap()
}

pub fn decode_base36(s: &str) -> Option<usize> {
    let mut n: usize = 0;
    for ch in s.chars() {
        let c = ch.to_ascii_uppercase() as u8;
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'A'..=b'Z' => c - b'A' + 10,
            _ => return None,
        };
        n = n.checked_mul(36)?.checked_add(d as usize)?;
    }
    Some(n)
}
