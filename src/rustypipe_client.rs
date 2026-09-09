use rustypipe::client::RustyPipe;

/// Store RustyPipe state in the operating system cache directory.
pub(crate) fn new() -> RustyPipe {
    let cache_dir = directories::ProjectDirs::from("", "", "ytamp")
        .map(|directories| directories.cache_dir().join("rustypipe"))
        .unwrap_or_else(|| std::env::temp_dir().join("ytamp").join("rustypipe"));
    if let Err(error) = std::fs::create_dir_all(&cache_dir) {
        eprintln!("Could not create RustyPipe cache directory: {error}");
    }
    RustyPipe::builder()
        .storage_dir(cache_dir)
        .build()
        .expect("RustyPipe accepts the application cache directory")
}
