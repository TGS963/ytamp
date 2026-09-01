//! Runs the sign-in check against the saved cookie file and prints
//! the exact failure with its full error chain.
//!
//! It runs the check twice: once on a plain runtime, once spawned on
//! a two-worker runtime the way the app does it.
//! Usage: cargo run --example auth_probe

use std::error::Error as StdError;
use std::fs;

use directories::ProjectDirs;

fn main() {
    let dirs = ProjectDirs::from("", "", "ytamp").expect("a config directory");
    let path = dirs.config_dir().join("cookies.txt");
    let cookies = fs::read_to_string(&path).expect("the saved cookie file");
    let cookies = cookies.trim().to_string();
    println!("cookie file: {} ({} bytes)", path.display(), cookies.len());

    let plain = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    println!("[plain runtime] {}", plain.block_on(check(&cookies)));

    let app_like = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let spawned = app_like.spawn(check_owned(cookies));
    println!(
        "[spawned, 2 workers] {}",
        app_like.block_on(spawned).expect("join")
    );
}

async fn check_owned(cookies: String) -> String {
    check(&cookies).await
}

async fn check(cookies: &str) -> String {
    match ytmapi_rs::YtMusic::from_cookie(cookies).await {
        Err(error) => format!("token build failed: {}", chain(&error)),
        Ok(yt) => match yt.get_library_playlists().await {
            Err(error) => format!("library check failed: {}", chain(&error)),
            Ok(playlists) => format!("signed in, {} playlists", playlists.len()),
        },
    }
}

/// The error and every source under it, joined for one line.
fn chain(error: &dyn StdError) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        parts.push(cause.to_string());
        source = cause.source();
    }
    parts.join(" <- ")
}
