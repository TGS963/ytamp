use serde_json::Value;

use super::DataApi;

pub(super) const BASE: &str = "https://www.googleapis.com/youtube/v3";

impl DataApi {
    pub fn new(token: ytmapi_rs::auth::OAuthToken) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|_| "Couldn’t create a network connection.".to_owned())?;
        Ok(Self {
            refresh_client: ytmapi_rs::Client::new_from_reqwest_client(http.clone()),
            http,
            token: tokio::sync::RwLock::new(token),
            auth_expired: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn needs_sign_in(&self) -> bool {
        self.auth_expired.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn verify_account(&self) -> Result<(), String> {
        self.send_with_retry(|http, token| {
            http.get(format!("{BASE}/playlists?part=id&mine=true&maxResults=1"))
                .bearer_auth(token)
        })
        .await
        .map(|_| ())
    }

    pub(super) async fn get_json(&self, url: &str) -> Result<Value, String> {
        self.send_with_retry(|http, token| http.get(url).bearer_auth(token))
            .await
    }

    pub(super) async fn post_json(&self, url: &str, body: Value) -> Result<Value, String> {
        self.send_with_retry(|http, token| http.post(url).bearer_auth(token).json(&body))
            .await
    }

    pub(super) async fn delete(&self, url: &str) -> Result<(), String> {
        self.send_with_retry(|http, token| http.delete(url).bearer_auth(token))
            .await?;
        Ok(())
    }

    pub(super) async fn send_with_retry(
        &self,
        build: impl Fn(&reqwest::Client, &str) -> reqwest::RequestBuilder,
    ) -> Result<Value, String> {
        let first = self.send_with_current_token(&build).await?;
        if first.status() != reqwest::StatusCode::UNAUTHORIZED {
            return parse_response(first).await;
        }
        self.refresh_token().await?;
        let second = self.send_with_current_token(&build).await?;
        if second.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.auth_expired
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        parse_response(second).await
    }

    async fn send_with_current_token(
        &self,
        build: &impl Fn(&reqwest::Client, &str) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, String> {
        let access_token = access_token_of(&*self.token.read().await)?;
        build(&self.http, &access_token)
            .send()
            .await
            .map_err(|error| format!("The request failed: {error}"))
    }

    async fn refresh_token(&self) -> Result<(), String> {
        let mut token = self.token.write().await;
        let fresh = token.refresh(&self.refresh_client).await.map_err(|error| {
            let message = error.to_string();
            if crate::core::sign_in::SignInFailure::classify(&message)
                == crate::core::sign_in::SignInFailure::Expired
            {
                self.auth_expired
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            format!("The OAuth refresh failed: {message}")
        })?;
        if let Ok(json) = serde_json::to_string(&fresh)
            && let Err(error) = crate::auth::save_oauth_token(&json)
        {
            log::warn!("could not persist the refreshed OAuth token: {error}");
        }
        *token = fresh;
        Ok(())
    }
}

fn access_token_of(token: &ytmapi_rs::auth::OAuthToken) -> Result<String, String> {
    serde_json::to_value(token)
        .ok()
        .and_then(|value| value.get("access_token")?.as_str().map(String::from))
        .ok_or("The OAuth token carries no access token.".to_string())
}

async fn parse_response(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if status == reqwest::StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("The response does not parse: {error}"))?;
    if let Some(error) = body.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(format!("YouTube answered {status}: {message}"));
    }
    if !status.is_success() {
        return Err(format!("YouTube answered {status}"));
    }
    Ok(body)
}
