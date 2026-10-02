use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};

use crate::ncm::{AudioQuality, DEFAULT_NCMEAPI_URL};

/// A Netease Cloud Music downloader implemented in Rust
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    // URL
    pub url: String,

    #[command(flatten)]
    pub verbosity: Verbosity<InfoLevel>,

    /// Path to Cookies TXT (netscape format)
    #[arg(short, long)]
    pub cookies: Option<String>,

    /// Sound quality used for downloading
    #[arg(short = 'Q', long, default_value_t = AudioQuality::ExHigh)]
    pub quality: AudioQuality,

    /// Maximum concurrent downloads
    #[arg(long, short = 'n', default_value_t = 4)]
    pub concurrent: usize,

    /// Maximum retry count
    #[arg(long, short = 'a', default_value_t = 3)]
    pub attempts: usize,

    /// Base Output Path
    #[arg(long, short = 'o', default_value = "out")]
    pub base_path: String,

    /// NCMEAPI base URL
    #[arg(long, default_value = DEFAULT_NCMEAPI_URL)]
    pub base_url: String,

    /// Album Folder Template
    #[arg(long, default_value = "{{ album.name|s }} ({{ album.release_year }})")]
    pub album_tmpl: String,

    /// Cover Template
    #[arg(long, default_value = "cover.{{ ext }}")]
    pub cover_tmpl: String,

    /// Song Template
    #[arg(
        long,
        default_value = "\
        {%- if song.disc_total > 1 %}\
            {{ song.disc|pad_to(song.disc_total) }}.\
        {% endif %}\
        {{ song.track|pad_to(song.track_total) }} \
        {{ song.name|s }}.{{ ext }}\
    "
    )]
    pub song_tmpl: String,
}
