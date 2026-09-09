//! Live read-only lyrics availability probe; prints no lyric text.
#[tokio::main]
async fn main() {
    let id = std::env::args().nth(1).expect("provide a video id");
    match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        ytamp::api::lyrics::fetch(&ytamp::core::model::TrackId(id)),
    )
    .await
    {
        Ok(Ok(Some(lyrics))) => println!(
            "PASS: lyrics available ({} characters, {} timed lines); attribution {}",
            lyrics.text.chars().count(),
            lyrics.timed_lines.len(),
            if lyrics.source.is_empty() {
                "absent"
            } else {
                "present"
            }
        ),
        Ok(Ok(None)) => println!("PASS: lyrics unavailable"),
        Ok(Err(error)) => {
            eprintln!("FAIL: {error}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("FAIL: request timed out");
            std::process::exit(1);
        }
    }
}
