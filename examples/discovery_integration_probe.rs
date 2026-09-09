//! Read-only production discovery/navigation check. No item titles or credentials are printed.
use ytamp::core::{discovery::Target, effect::AuthMethod};
#[tokio::main]
async fn main() {
    let Some(token) = ytamp::auth::load_oauth_token() else {
        println!("OAuth sign-in required");
        return;
    };
    let Ok(api) = ytamp::api::Api::sign_in(&AuthMethod::OAuthToken(token)).await else {
        println!("OAuth sign-in failed; renew sign-in in ytamp");
        return;
    };
    let mut cursor = None;
    let mut targets = vec![];
    for page in 1..=3 {
        let Ok(feed) = api.discovery(&Target::home(), cursor).await else {
            println!("Page {page}: request failed");
            return;
        };
        println!(
            "Page {page}: {} shelves, {} cards",
            feed.shelves.len(),
            feed.shelves.iter().map(|s| s.entries.len()).sum::<usize>()
        );
        for shelf in feed.shelves {
            for entry in shelf.entries {
                if entry.track.is_none() {
                    targets.push(entry.target);
                }
            }
        }
        cursor = feed.continuation;
        if cursor.is_none() {
            break;
        }
    }
    let mut kinds = std::collections::BTreeSet::new();
    for target in targets {
        let kind = match &target {
            Target::Browse { id, .. } => {
                if id.starts_with("VLRD") {
                    "mix"
                } else if id.starts_with("VLOLAK") {
                    "album"
                } else if id.starts_with("VL") {
                    "playlist"
                } else {
                    "artist"
                }
            }
            Target::Watch { .. } => "radio",
        };
        if !kinds.insert(kind) {
            continue;
        }
        match api.discovery(&target, None).await {
            Ok(feed) => {
                println!(
                    "{kind}: {} tracks, {} shelves",
                    feed.tracks.len(),
                    feed.shelves.len()
                );
                if kind == "mix" && std::env::args().any(|arg| arg == "--decode") {
                    let track = feed.tracks.first().expect("mix has playable tracks");
                    tokio::time::timeout(
                        std::time::Duration::from_secs(60),
                        verify_audio(&track.id.0),
                    )
                    .await
                    .expect("audio check timed out");
                }
            }
            Err(_) => println!("{kind}: request or parser failed"),
        }
    }
}

// Exercises the real resolver/download/decoder without touching the application's
// queue or sending audio to the speakers. Opt in with --decode.
async fn verify_audio(id: &str) {
    use std::{sync::Arc, time::Duration};
    let resolvers = Arc::new(ytamp::stream::ResolverChain::with_default_resolvers());
    let buffer =
        ytamp::stream::disk_cache::fetch_audio(&resolvers, &reqwest::Client::new(), id).await;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let decoder = ytamp::player::source::spawn_decoder(buffer, move |ready| {
        let _ = sender.send(ready);
    });
    let ready = receiver
        .await
        .expect("decoder report")
        .expect("valid audio stream");
    let (mut source, position) = decoder.into_source(ready);
    while position.position() < Duration::from_secs(2) {
        for _ in 0..1000 {
            assert!(
                source.next().is_some(),
                "mix audio ended before two seconds"
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    println!("mix audio: decoded two seconds; playback position advances");
}
