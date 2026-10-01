mod auth;
mod cli;
mod ncm;
mod util;
mod model;
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
    let client = ncm::client::NCMClient::new(
        args.cookies,
        args.attempts,
        args.concurrent,
        args.base_url,
    );
    // info!("\nDETAIL: \n{:#?}",client.song_detail(&[3410966242]).await.unwrap());
    // info!("\nURL: \n{:#?}",client.song_url(&[3410966242], ncm::AudioQuality::ExHigh).await.unwrap());
    // info!("DECODED:\n{:#?}", client.album(388843799).await.unwrap());
    let mut downloader = model::Downloader::new(client);
    downloader.pull_album_info(388843799).await;
    downloader.pull_album_play_urls(388843799, args.quality).await;
    downloader.pull_album_lyrics(388843799).await;
    info!("Songs: {:#?}",downloader.lib.song_pools);
    info!("Albums: {:#?}",downloader.lib.album_pools);
}
