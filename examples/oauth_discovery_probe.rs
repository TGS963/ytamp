//! Read-only discovery capability check. Prints no credentials or item titles.
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
    let http = reqwest::Client::builder().user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36").build().unwrap();
    let page = http
        .get("https://music.youtube.com/")
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let html = match page {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(_) => String::new(),
    };
    let version = config_string(&html, "INNERTUBE_CLIENT_VERSION")
        .unwrap_or_else(|| "1.20260908.01.00".into());
    let visitor = config_string(&html, "VISITOR_DATA").unwrap_or_default();
    println!(
        "Live Music config: version found {}, visitor found {}",
        config_string(&html, "INNERTUBE_CLIENT_VERSION").is_some(),
        !visitor.is_empty()
    );
    for (client, version, host, browse) in [
        (
            "WEB_REMIX",
            version.as_str(),
            "music.youtube.com",
            "FEmusic_home",
        ),
        (
            "WEB_REMIX",
            version.as_str(),
            "music.youtube.com",
            "FEmusic_explore",
        ),
        (
            "WEB_REMIX",
            version.as_str(),
            "music.youtube.com",
            "FEmusic_moods_and_genres",
        ),
        (
            "TVHTML5",
            "7.20240925.00.00",
            "www.youtube.com",
            "FEwhat_to_watch",
        ),
        (
            "TVHTML5",
            "7.20240925.00.00",
            "www.youtube.com",
            "FEtopics_music",
        ),
        (
            "TVHTML5",
            "7.20240925.00.00",
            "www.youtube.com",
            "FEmusic_new_releases_videos",
        ),
        (
            "WEB_REMIX",
            version.as_str(),
            "music.youtube.com",
            "FEmusic_charts",
        ),
        (
            "TVHTML5",
            "7.20240925.00.00",
            "www.youtube.com",
            "UC-9-kyTW8ZkZNDHQJ6FgpwQ",
        ),
    ] {
        if std::env::args().any(|a| a == "--tv-music") && browse != "FEtopics_music" {
            continue;
        }
        let request = http.post(format!("https://{host}/youtubei/v1/browse"))
            .bearer_auth(&access)
            .header("Origin",format!("https://{host}"))
            .header("X-Goog-Request-Time", "1")
            .json(&json!({"context":{"client":{"clientName":client,"clientVersion":version,"hl":"en","gl":"US","visitorData":visitor}},"browseId":browse}))
            .timeout(Duration::from_secs(15));
        let request = if client == "WEB_REMIX" {
            request
                .header("X-Youtube-Client-Name", "67")
                .header("X-Youtube-Client-Version", version)
                .header("X-Goog-Visitor-Id", &visitor)
        } else {
            request.header(
                "User-Agent",
                "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version",
            )
        };
        let result = request.send().await;
        match result {
            Ok(response) => {
                let status = response.status().as_u16();
                let value: Value = response.json().await.unwrap_or(Value::Null);
                let mut headings = std::collections::BTreeSet::new();
                headings_from(&value, &mut headings);
                println!(
                    "{client}/{browse}: HTTP {status}, contents {}, video references {}, sign-in prompt {}, sections {:?}",
                    value.get("contents").is_some(),
                    count_videos(&value),
                    contains_sign_in(&value),
                    headings
                );
                if status == 200 {
                    let mut kinds = std::collections::BTreeMap::new();
                    renderer_kinds(&value, &mut kinds);
                    println!("  renderers: {kinds:?}");
                    if browse == "FEtopics_music" {
                        let mut cursor = find_continuation(&value).map(str::to_owned);
                        for page in 2..=4 {
                            let Some(continuation) = cursor.take() else {
                                break;
                            };
                            let response=http.post(format!("https://{host}/youtubei/v1/browse"))
                                .bearer_auth(&access).header("User-Agent","Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version")
                                .json(&json!({"context":{"client":{"clientName":client,"clientVersion":version,"hl":"en","gl":"US"}},"continuation":continuation}))
                                .timeout(Duration::from_secs(15)).send().await;
                            if let Ok(response) = response {
                                let status = response.status().as_u16();
                                let value: Value = response.json().await.unwrap_or(Value::Null);
                                let mut headings = std::collections::BTreeSet::new();
                                headings_from(&value, &mut headings);
                                let mut kinds = std::collections::BTreeMap::new();
                                renderer_kinds(&value, &mut kinds);
                                println!(
                                    "  page {page}: HTTP {status}, sections {headings:?}, renderers {kinds:?}"
                                );
                                cursor = find_continuation(&value).map(str::to_owned);
                            }
                        }
                    }
                }
                if status != 200 {
                    println!(
                        "  error: {}",
                        value
                            .pointer("/error/status")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                    );
                }
            }
            Err(_) => println!("{client}/{browse}: request failed"),
        }
    }
}
// Only shelf headings; never print tokens, visitor data, or item titles.
fn headings_from(value: &Value, out: &mut std::collections::BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if [
                    "musicCarouselShelfBasicHeaderRenderer",
                    "shelfHeaderRenderer",
                ]
                .contains(&key.as_str())
                {
                    let mut texts = Vec::new();
                    rendered_text(value, &mut texts);
                    if !texts.is_empty() {
                        out.insert(texts.join(" "));
                    }
                }
                headings_from(value, out);
            }
        }
        Value::Array(values) => {
            for value in values {
                headings_from(value, out);
            }
        }
        _ => (),
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

fn config_string(html: &str, key: &str) -> Option<String> {
    let (_, rest) = html.split_once(&format!("\"{key}\":"))?;
    serde_json::Deserializer::from_str(rest.trim_start())
        .into_iter::<Value>()
        .next()?
        .ok()?
        .as_str()
        .map(str::to_owned)
}
fn renderer_kinds(value: &Value, out: &mut std::collections::BTreeMap<String, usize>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k.ends_with("Renderer") {
                    *out.entry(k.clone()).or_default() += 1;
                }
                renderer_kinds(v, out);
            }
        }
        Value::Array(values) => {
            for v in values {
                renderer_kinds(v, out);
            }
        }
        _ => (),
    }
}

fn rendered_text(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if ["text", "simpleText", "content"].contains(&k.as_str())
                    && let Some(text) = v.as_str()
                {
                    out.push(text.into());
                }
                rendered_text(v, out);
            }
        }
        Value::Array(values) => {
            for v in values {
                rendered_text(v, out);
            }
        }
        _ => (),
    }
}
fn find_continuation(value: &Value) -> Option<&str> {
    if let Some(c) = value
        .pointer("/sectionListRenderer/continuations/0/nextContinuationData/continuation")
        .and_then(Value::as_str)
    {
        return Some(c);
    }
    if let Some(c) = value
        .pointer("/sectionListContinuation/continuations/0/nextContinuationData/continuation")
        .and_then(Value::as_str)
    {
        return Some(c);
    }
    match value {
        Value::Object(map) => map.values().find_map(find_continuation),
        Value::Array(values) => values.iter().find_map(find_continuation),
        _ => None,
    }
}
