use super::{Action, Api, ApiSlot, EffectRuntime};
use crate::auth;

impl EffectRuntime {
    pub(super) fn load_stored_auth(&self) {
        let deliver = self.scoped_delivery();
        self.tokio.spawn_blocking(move || {
            deliver(match auth::load_auth_method() {
                Some(method) => Action::StoredAuthFound(method),
                None => Action::AuthVerified(Err(crate::core::sign_in::SignInFailure::Expired)),
            });
        });
    }

    pub(super) fn clear_credentials(&self) {
        self.api.write().expect("api lock").api = None;
        let deliver = self.delivery();
        {
            if let Err(error) = auth::delete_credentials() {
                deliver(Action::NoticePosted(format!("Signing out failed: {error}")));
            }
        }
    }
}

pub(super) async fn sign_in(
    slot: &ApiSlot,
    generation: u64,
    method: &crate::core::effect::AuthMethod,
) -> Action {
    match Api::sign_in(method).await {
        Ok(api) => {
            let mut guard = slot.write().expect("api lock");
            if guard.generation == generation {
                guard.api = Some(api);
            }
            Action::AuthVerified(Ok(()))
        }
        Err(message) => Action::AuthVerified(Err(message)),
    }
}

/// How long and how often the flow polls Google while the user
/// finishes the sign-in in the browser. Five seconds is the device
/// flow's standard interval.
const OAUTH_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

const OAUTH_POLL_ATTEMPTS: u32 = 60;

/// The OAuth device flow: get a device code, hand the verification URL
/// to the UI, poll until the user finishes, then store the token and
/// sign in with it.
pub(super) async fn run_oauth_flow(
    slot: &ApiSlot,
    generation: u64,
    client_id: String,
    client_secret: String,
    deliver: &(impl Fn(Action) + Send),
) {
    let result = oauth_token_from_device_flow(&client_id, &client_secret, deliver).await;
    let action = match result {
        Ok(token_json) => {
            {
                let guard = slot.read().expect("api lock");
                if guard.generation != generation {
                    return;
                }
                if let Err(error) = auth::save_oauth_token(&token_json) {
                    deliver(Action::NoticePosted(format!(
                        "Saving the sign-in failed: {error}"
                    )));
                } else {
                    deliver(Action::OAuthTokenStored);
                }
            }
            sign_in(
                slot,
                generation,
                &crate::core::effect::AuthMethod::OAuthToken(token_json),
            )
            .await
        }
        Err(message) => Action::AuthVerified(Err(message.into())),
    };
    deliver(action);
}

pub(super) async fn oauth_token_from_device_flow(
    client_id: &str,
    client_secret: &str,
    deliver: &(impl Fn(Action) + Send),
) -> Result<String, String> {
    let client = ytmapi_rs::Client::new()
        .map_err(|error| format!("The HTTP client failed to build: {error}"))?;
    let (code, url) = ytmapi_rs::generate_oauth_code_and_url(&client, client_id)
        .await
        .map_err(|error| format!("The OAuth start failed: {error}. Check the client id."))?;
    deliver(Action::OAuthUrlReady(url));
    let mut last_error = String::new();
    for _ in 0..OAUTH_POLL_ATTEMPTS {
        tokio::time::sleep(OAUTH_POLL_INTERVAL).await;
        match ytmapi_rs::generate_oauth_token(&client, code.clone(), client_id, client_secret).await
        {
            Ok(token) => {
                return serde_json::to_string(&token)
                    .map_err(|error| format!("The token does not serialize: {error}"));
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(format!("The sign-in did not finish in time: {last_error}"))
}
