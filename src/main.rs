mod auth;
mod cli;
mod export;
mod model;
mod ncm;
mod tag;
mod util;
use std::path::PathBuf;

use clap::Parser;
use cli::Args;
use tracing::info;

#[tokio::main]
async fn main() {
    let args = Args::parse();
    // tls provider = ring
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    tracing_subscriber::fmt()
        .with_max_level(args.verbosity)
        .init();
    info!(
        "quality: {} ({})",
        args.quality.label(),
        args.quality.field()
    );
    info!("cookies: {:?}", args.cookies); // load_cookie
    let client =
        ncm::client::NCMClient::new(args.cookies, args.attempts, args.concurrent, args.base_url);
    // info!("\nDETAIL: \n{:#?}",client.song_detail(&[3410966242]).await.unwrap());
    // info!("\nURL: \n{:#?}",client.song_url(&[3410966242], ncm::AudioQuality::ExHigh).await.unwrap());
    // info!("DECODED:\n{:#?}", client.album(388843799).await.unwrap());
    let album_id = client.get_album_id(&args.url).await.unwrap();
    let mut downloader = model::Downloader::new(client);
    downloader.pull_album_info(album_id).await;
    downloader
        .pull_album_play_urls(album_id, args.quality)
        .await;
    downloader.pull_album_lyrics(album_id).await;

    let exporter = export::Exporter::new(downloader);

    let album_folder = "{{ album.name|s }} ({{ album.release_year }})";
    let cover_tmpl = format!("{}{}", album_folder, "/cover.{{ ext }}");
    let song_tmpl = format!(
        "{}{}",
        album_folder,
        "/\
    {%- if song.disc_total > 1 %}\
        {{ song.disc|pad_to(song.disc_total) }}.\
    {% endif %}\
    {{ song.track|pad_to(song.track_total) }} \
    {{ song.name|s }}.{{ ext }}\
    "
    );
    let base = PathBuf::from("out");

    let results = exporter
        .export_album(album_id, Some(&cover_tmpl), &song_tmpl, &base)
        .await
        .unwrap();

    println!();
    for (sid, path) in results.songs {
        println!("{}", path.display());
    }
}
