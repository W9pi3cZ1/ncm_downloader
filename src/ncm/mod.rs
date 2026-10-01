pub mod client;
pub mod dto;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const DEFAULT_NCMEAPI_URL: &str = "https://ncmapi.xslimenb.eu.org";

#[derive(ValueEnum, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "lower")]
#[serde(rename_all = "lowercase")]
pub enum AudioQuality {
    None,
    Standard,
    Higher,
    ExHigh,
    Lossless,
    HiRes,
    JyEffect,
    Sky,
    Dolby,
    JyMaster,
}

impl AudioQuality {
    pub fn label(self) -> &'static str{
        match self {
            Self::None => "无权限",
            Self::Standard => "标准",
            Self::Higher => "较高",
            Self::ExHigh => "极高",
            Self::Lossless => "无损",
            Self::HiRes => "Hi-Res",
            Self::JyEffect => "高清环绕声",
            Self::Sky => "沉浸环绕声",
            Self::Dolby => "杜比全景声",
            Self::JyMaster => "超清母带",
        }
    }

    pub fn field(self) -> &'static str{
        match self {
            Self::None => "none",
            Self::Standard => "standard",
            Self::Higher => "higher",
            Self::ExHigh => "exhigh",
            Self::Lossless => "lossless",
            Self::HiRes => "hires",
            Self::JyEffect => "jyeffect",
            Self::Sky => "sky",
            Self::Dolby => "dolby",
            Self::JyMaster => "jymaster",
        }
    }
}

impl fmt::Display for AudioQuality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.field())
    }
}