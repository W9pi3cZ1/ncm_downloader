use crate::model::lyric::{Lyrics, parse_lrc};
use std::collections::BTreeMap;

impl Lyrics {
    /// 输出为 TTML，采用 inline auxiliary 结构：
    /// 每个时间戳对应一个 `<p>`，翻译/罗马音作为 `<span ttm:role="...">` 内联。
    pub fn to_ttml(&self) -> String {
        let mut out = String::with_capacity(1024);
        out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        out.push('\n');
        out.push_str(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata">"#,
        );
        out.push('\n');
        out.push_str("  <body>\n");
        out.push_str(r#"    <div xml:lang="und">"#);
        out.push('\n');

        // 三段各自解析，按时间戳归并
        let mut by_time: BTreeMap<u64, (Vec<String>, Vec<String>, Vec<String>)> = BTreeMap::new();

        let push =
            |src: Option<&String>,
             idx: usize,
             map: &mut BTreeMap<u64, (Vec<String>, Vec<String>, Vec<String>)>| {
                let Some(s) = src else { return };
                for (t, text) in parse_lrc(s) {
                    if text.is_empty() {
                        continue;
                    }
                    let entry = map.entry(t).or_default();
                    let bucket = match idx {
                        0 => &mut entry.0,
                        1 => &mut entry.1,
                        _ => &mut entry.2,
                    };
                    if !bucket.contains(&text) {
                        bucket.push(text);
                    }
                }
            };

        push(self.orig.as_ref(), 0, &mut by_time);
        push(self.trans.as_ref(), 1, &mut by_time);
        push(self.roma.as_ref(), 2, &mut by_time);

        let entries: Vec<(u64, (Vec<String>, Vec<String>, Vec<String>))> =
            by_time.into_iter().collect();

        for (i, (start, (orig, trans, roma))) in entries.iter().enumerate() {
            let end = entries.get(i + 1).map(|(t, _)| *t).unwrap_or(start + 5_000);

            out.push_str(&format!(
                "      <p begin=\"{}\" end=\"{}\">\n",
                fmt_ttml_ts(*start),
                fmt_ttml_ts(end)
            ));

            // 原文（作为主歌词）
            for v in orig {
                out.push_str(&format!("        {}\n", xml_escape(v)));
            }
            // 翻译
            for v in trans {
                out.push_str(&format!(
                    "        <span ttm:role=\"x-translation\" xml:lang=\"zh-Hans\">{}</span>\n",
                    xml_escape(v)
                ));
            }
            // 罗马音
            for v in roma {
                out.push_str(&format!(
                    "        <span ttm:role=\"x-roman\" xml:lang=\"ja-Latn\">{}</span>\n",
                    xml_escape(v)
                ));
            }

            out.push_str("      </p>\n");
        }

        out.push_str("    </div>\n");
        out.push_str("  </body>\n");
        out.push_str("</tt>\n");
        out
    }
}

fn fmt_ttml_ts(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let f = ms % 1000;
    format!("{h:02}:{m:02}:{s:02}.{f:03}")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
