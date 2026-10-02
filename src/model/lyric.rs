use std::collections::HashMap;

use crate::{
    ncm::{client::Timed, dto},
    util::non_empty,
};

fn parse_lrc_timestamp(s: &str) -> Option<u64> {
    // 支持 mm:ss.xx / mm:ss.xxx / mm:ss:xx（网易云偶发）
    let parts: Vec<&str> = s.split(|c| c == ':' || c == '.').collect();
    if parts.len() < 2 {
        return None;
    }
    let min: u64 = parts[0].trim().parse().ok()?;
    let sec: u64 = parts[1].trim().parse().ok()?;
    if sec >= 60 {
        return None;
    }
    let frac_ms: u64 = if parts.len() >= 3 {
        let f = parts[2].trim();
        let val: u64 = f.parse().ok()?;
        match f.len() {
            1 => val * 100,
            2 => val * 10,
            3 => val,
            _ => return None,
        }
    } else {
        0
    };
    Some(min * 60_000 + sec * 1000 + frac_ms)
}

pub fn parse_lrc(lrc: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    for line in lrc.lines() {
        let mut timestamps = Vec::new();
        let mut rest = line;
        loop {
            let trimmed = rest.trim_start();
            if !trimmed.starts_with('[') {
                rest = trimmed;
                break;
            }
            let Some(end) = trimmed.find(']') else { break };
            let tag = &trimmed[1..end];
            if let Some(ms) = parse_lrc_timestamp(tag) {
                timestamps.push(ms);
            }
            rest = &trimmed[end + 1..];
        }
        let text = rest.trim().to_string();
        for t in timestamps {
            out.push((t, text.clone()));
        }
    }
    out.sort_by_key(|(t, _)| *t);
    out
}

fn format_lrc_timestamp(ms: u64) -> String {
    let min = ms / 60_000;
    let sec = (ms % 60_000) / 1000;
    let cs = (ms % 1000) / 10;
    format!("[{:02}:{:02}.{:02}]", min, sec, cs)
}

#[derive(Debug, Clone)]
pub struct Lyrics {
    pub orig: Option<String>,
    pub trans: Option<String>,
    pub roma: Option<String>,
    pub mix: Option<String>,
}

impl From<Timed<dto::lyric::API>> for Lyrics {
    fn from(value: Timed<dto::lyric::API>) -> Self {
        let lrc = value.value.lrc.and_then(|x| Some(x.lyric));
        let tlyric = value.value.tlyric.and_then(|x| Some(x.lyric));
        let romalrc = value.value.romalrc.and_then(|x| Some(x.lyric));
        Self::merge_lyrics(
            lrc.as_ref().map(|x| x.as_str()),
            tlyric.as_ref().map(|x| x.as_str()),
            romalrc.as_ref().map(|x| x.as_str()),
        )
    }
}

impl Lyrics {
    pub fn new() -> Self {
        Self {
            orig: None,
            trans: None,
            roma: None,
            mix: None,
        }
    }

    /// 原文 + 罗马音 + 翻译，同一时间戳分行输出
    pub fn merge_lyrics(lrc: Option<&str>, tlyric: Option<&str>, romalrc: Option<&str>) -> Self {
        let mut orig_out = String::new();
        let mut trans_out = String::new();
        let mut roma_out = String::new();
        let mut mix_out = String::new();
        if let Some(l) = lrc {
            let orig = parse_lrc(l);
            let trans: Option<HashMap<u64, String>> =
                tlyric.and_then(|x| Some(parse_lrc(x).into_iter().collect()));
            let roma: Option<HashMap<u64, String>> =
                romalrc.and_then(|x| Some(parse_lrc(x).into_iter().collect()));

            for (t, text) in &orig {
                let ts = format_lrc_timestamp(*t);

                // 原文
                orig_out.push_str(&ts);
                orig_out.push_str(text);
                orig_out.push('\n');
                mix_out.push_str(&ts);
                mix_out.push_str(text);
                mix_out.push('\n');

                // 罗马音
                if let Some(r) = roma.as_ref().and_then(|x| x.get(t)) {
                    if !r.is_empty() && r != text {
                        roma_out.push_str(&ts);
                        roma_out.push_str(r);
                        roma_out.push('\n');
                        mix_out.push_str(&ts);
                        mix_out.push_str(r);
                        mix_out.push('\n');
                    }
                }

                // 翻译
                if let Some(tr) = trans.as_ref().and_then(|x| x.get(t)) {
                    if !tr.is_empty() && tr != text {
                        trans_out.push_str(&ts);
                        trans_out.push_str(tr);
                        trans_out.push('\n');
                        mix_out.push_str(&ts);
                        mix_out.push_str(tr);
                        mix_out.push('\n');
                    }
                }
            }
        }
        Self {
            orig: non_empty(orig_out),
            trans: non_empty(trans_out),
            roma: non_empty(roma_out),
            mix: non_empty(mix_out),
        }
    }
}
