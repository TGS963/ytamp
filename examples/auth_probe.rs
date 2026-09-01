//! Runs the sign-in check against the saved cookie file and prints
//! the exact failure. Usage: cargo run --example auth_probe

use std::fs;

use directories::ProjectDirs;

#[tokio::main]
async fn main() {
    let dirs = ProjectDirs::from("", "", "ytamp").expect("a config directory");
    let path = dirs.config_dir().join("cookies.txt");
    let cookies = fs::read_to_string(&path).expect("the saved cookie file");
    println!("cookie file: {} ({} bytes)", path.display(), cookies.len());

    match ytmapi_rs::YtMusic::from_cookie(cookies.trim()).await {
        Err(error) => println!("token build failed: {error}"),
        Ok(yt) => match yt.get_library_playlists().await {
            Err(error) => println!("library check failed: {error}"),
            Ok(playlists) => println!("signed in, {} playlists", playlists.len()),
        },
    }
}
