use std::time::Duration;

pub struct OAuthSession {
    pub access_token: String,
    // Each example compiles this shared module separately. Only history needs this field.
    #[allow(dead_code)]
    pub token_json: String,
}

pub enum OAuthSessionError {
    Missing,
    Invalid,
    RefreshFailed,
    MissingAccess,
}

/// Load and refresh the saved OAuth token without exposing its contents.
pub async fn oauth_session() -> Result<OAuthSession, OAuthSessionError> {
    let json = ytamp::auth::load_oauth_token().ok_or(OAuthSessionError::Missing)?;
    let token = serde_json::from_str::<ytmapi_rs::auth::OAuthToken>(&json)
        .map_err(|_| OAuthSessionError::Invalid)?;
    let mut client = ytmapi_rs::YtMusic::from_auth_token(token);
    let token = tokio::time::timeout(Duration::from_secs(15), client.refresh_token())
        .await
        .map_err(|_| OAuthSessionError::RefreshFailed)?
        .map_err(|_| OAuthSessionError::RefreshFailed)?;
    let token_json = serde_json::to_string(&token).map_err(|_| OAuthSessionError::MissingAccess)?;
    let access_token = serde_json::from_str::<serde_json::Value>(&token_json)
        .map_err(|_| OAuthSessionError::MissingAccess)?
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or(OAuthSessionError::MissingAccess)?;
    Ok(OAuthSession {
        access_token,
        token_json,
    })
}
