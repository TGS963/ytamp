use serde_json::Value;

use super::DataApi;

const TV_ENDPOINT: &str = "https://www.youtube.com/youtubei/v1";
const TV_USER_AGENT: &str = "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version";

impl DataApi {
    pub async fn discovery(
        &self,
        target: &crate::core::discovery::Target,
        continuation: Option<String>,
    ) -> Result<crate::core::discovery::FeedPage, String> {
        let (endpoint, body) = discovery_request(target, continuation);
        let value = self.send_tv_json(endpoint, &body).await?;
        parse_discovery_response(target, &value)
    }

    pub async fn history(
        &self,
        continuation: Option<String>,
    ) -> Result<crate::core::listening_history::HistoryPage, String> {
        let body = history_request(continuation);
        let value = self.send_tv_text("browse", &body).await?;
        super::super::history::parse(&value)
    }

    async fn send_tv_json(&self, endpoint: &str, body: &Value) -> Result<Value, String> {
        self.send_with_retry(|http, token| {
            http.post(format!("{TV_ENDPOINT}/{endpoint}"))
                .bearer_auth(token)
                .header("User-Agent", TV_USER_AGENT)
                .timeout(std::time::Duration::from_secs(15))
                .json(body)
        })
        .await
    }

    async fn send_tv_text(&self, endpoint: &str, body: &Value) -> Result<Value, String> {
        self.send_with_retry(|http, token| {
            http.post(format!("{TV_ENDPOINT}/{endpoint}"))
                .bearer_auth(token)
                .header("User-Agent", TV_USER_AGENT)
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(15))
                .body(body.to_string())
        })
        .await
    }
}

fn discovery_request(
    target: &crate::core::discovery::Target,
    continuation: Option<String>,
) -> (&'static str, Value) {
    let mut body = tv_context();
    let endpoint = add_discovery_target(&mut body, target);
    if let Some(continuation) = continuation {
        body = serde_json::json!({"context": body["context"], "continuation": continuation});
    }
    (endpoint, body)
}

fn add_discovery_target(body: &mut Value, target: &crate::core::discovery::Target) -> &'static str {
    use crate::core::discovery::Target;

    match target {
        Target::Browse { id, params } => {
            body["browseId"] = id.clone().into();
            if let Some(params) = params {
                body["params"] = params.clone().into();
            }
            "browse"
        }
        Target::Watch {
            video,
            playlist,
            params,
        } => {
            if let Some(video) = video {
                body["videoId"] = video.clone().into();
            }
            if let Some(playlist) = playlist {
                body["playlistId"] = playlist.clone().into();
            }
            if let Some(params) = params {
                body["params"] = params.clone().into();
            }
            "next"
        }
    }
}

fn history_request(continuation: Option<String>) -> Value {
    let mut body = tv_context();
    match continuation {
        Some(continuation) => body["continuation"] = Value::String(continuation),
        None => body["browseId"] = Value::String("FEhistory".into()),
    }
    body
}

fn tv_context() -> Value {
    serde_json::json!({"context":{"client":{"clientName":"TVHTML5","clientVersion":"7.20240925.00.00","hl":"en","gl":"US"}}})
}

fn parse_discovery_response(
    target: &crate::core::discovery::Target,
    value: &Value,
) -> Result<crate::core::discovery::FeedPage, String> {
    if let crate::core::discovery::Target::Watch {
        playlist: Some(playlist),
        ..
    } = target
    {
        super::super::discovery::parse_radio(value, playlist)
    } else {
        super::super::discovery::parse(value)
    }
}
