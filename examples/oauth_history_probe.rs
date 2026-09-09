//! Read-only OAuth capability check. Prints no credentials or history entries.
use serde_json::{Value, json};
use std::time::Duration;
mod probe_support;
#[tokio::main]
async fn main() {
    let session = match probe_support::oauth_session().await {
        Ok(session) => session,
        Err(probe_support::OAuthSessionError::Missing) => {
            println!("No saved OAuth token");
            return;
        }
        Err(probe_support::OAuthSessionError::Invalid) => {
            println!("Invalid saved token");
            return;
        }
        Err(probe_support::OAuthSessionError::RefreshFailed) => {
            println!("Token refresh failed or timed out");
            return;
        }
        Err(probe_support::OAuthSessionError::MissingAccess) => return,
    };
    let access = session.access_token;
    let token = session.token_json;
    let http = reqwest::Client::new();
    let control = http
        .get("https://www.googleapis.com/youtube/v3/playlistItems")
        .query(&[("playlistId", "LL"), ("part", "id"), ("maxResults", "1")])
        .bearer_auth(&access)
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    match control {
        Ok(response) => println!(
            "Official liked-songs control: HTTP {}",
            response.status().as_u16()
        ),
        Err(_) => println!("Control request failed"),
    }
    for (client, version, host, browse) in [
        (
            "WEB_REMIX",
            "1.20260908.01.00",
            "music.youtube.com",
            "FEmusic_history",
        ),
        ("IOS_MUSIC", "6.42", "music.youtube.com", "FEmusic_history"),
        (
            "TVHTML5",
            "7.20240925.00.00",
            "www.youtube.com",
            "FEmusic_history",
        ),
        (
            "TVHTML5",
            "7.20240925.00.00",
            "www.youtube.com",
            "FEhistory",
        ),
    ] {
        let ua = if client == "TVHTML5" {
            "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version"
        } else {
            "Mozilla/5.0"
        };
        let result = http.post(format!("https://{host}/youtubei/v1/browse"))
            .bearer_auth(&access).header("User-Agent", ua).header("Content-Type", "application/json")
            .body(json!({"context":{"client":{"clientName":client,"clientVersion":version,"hl":"en","gl":"US"}},"browseId":browse}).to_string())
            .timeout(Duration::from_secs(15)).send().await;
        match result {
            Ok(response) => {
                let status = response.status().as_u16();
                let Ok(bytes) = response.bytes().await else {
                    println!("{client}/{browse}: body failed");
                    continue;
                };
                let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                println!(
                    "{client}/{browse}: HTTP {status}, content {}, video entries {}, sign-in prompt {}",
                    value.get("contents").is_some(),
                    count_videos(&value),
                    contains_sign_in(&value)
                );
            }
            Err(_) => println!("{client}/{browse}: request failed"),
        }
    }
    let method = ytamp::core::effect::AuthMethod::OAuthToken(token.to_string());
    match ytamp::api::Api::sign_in(&method).await {
        Ok(api) => {
            let mut cursor = None;
            for page in 1..=3 {
                match api.history(cursor).await {
                    Ok(history) => {
                        println!(
                            "Production page {page}: {} tracks, {} with artists, {} with duration, older {}",
                            history.tracks.len(),
                            history
                                .tracks
                                .iter()
                                .filter(|t| !t.artists.is_empty())
                                .count(),
                            history
                                .tracks
                                .iter()
                                .filter(|t| t.duration.is_some())
                                .count(),
                            history.continuation.is_some()
                        );
                        cursor = history.continuation;
                        if cursor.is_none() {
                            break;
                        }
                    }
                    Err(_) => {
                        println!("Production page {page}: fetch or parser failed");
                        break;
                    }
                }
            }
        }
        Err(_) => println!("Production OAuth sign-in check failed"),
    }
}
fn count_videos(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            usize::from(map.get("videoId").is_some())
                + map.values().map(count_videos).sum::<usize>()
        }
        Value::Array(values) => values.iter().map(count_videos).sum(),
        _ => 0,
    }
}
fn contains_sign_in(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.contains_key("signInEndpoint") || map.values().any(contains_sign_in)
        }
        Value::Array(values) => values.iter().any(contains_sign_in),
        _ => false,
    }
}
