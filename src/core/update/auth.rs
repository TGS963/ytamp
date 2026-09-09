use super::{Action, ApiRequest, AuthState, Effect, Page, PlayerCommand, State, navigate};

pub(super) fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::OAuthTokenStored => {
            state.sign_in.saved_account = true;
            vec![]
        }
        Action::SignInCancelled => {
            state.session_generation += 1;
            state.sign_in.oauth_url = None;
            state.auth = if state.sign_in.saved_account {
                AuthState::ConnectionFailed
            } else {
                AuthState::SignedOut
            };
            vec![]
        }
        Action::SignInRetryRequested => {
            if !state.sign_in.saved_account || state.auth == AuthState::Verifying {
                return vec![];
            }
            state.session_generation += 1;
            state.auth = AuthState::Verifying;
            state.sign_in.oauth_url = None;
            vec![Effect::LoadStoredAuth]
        }
        Action::SessionExpired => {
            if state.auth == AuthState::SignedIn {
                state.auth = AuthState::Expired;
                state.sign_in.saved_account = true;
            }
            vec![]
        }
        Action::SignOutRequested => sign_out(state),
        Action::OAuthClientIdChanged(draft) => {
            state.sign_in.client_id_draft = draft;
            vec![]
        }
        Action::OAuthClientSecretChanged(draft) => {
            state.sign_in.client_secret_draft = draft;
            vec![]
        }
        Action::OAuthStartRequested => start_oauth(state),
        Action::OAuthUrlReady(url) => {
            state.sign_in.oauth_url = Some(url);
            vec![]
        }
        Action::AuthVerified(result) => finish_sign_in(state, result),
        Action::StoredAuthFound(method) => {
            state.sign_in.saved_account = true;
            let crate::core::effect::AuthMethod::OAuthToken(json) = &method;
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
                state.sign_in.client_id_draft = value
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .into();
                state.sign_in.client_secret_draft = value
                    .get("client_secret")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .into();
            }
            state.session_generation += 1;
            state.auth = AuthState::Verifying;
            vec![Effect::Api(ApiRequest::VerifyAuth(method))]
        }
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}

pub(super) fn start_oauth(state: &mut State) -> Vec<Effect> {
    if state.auth == AuthState::Verifying {
        return vec![];
    }
    let client_id = state.sign_in.client_id_draft.trim().to_string();
    let client_secret = state.sign_in.client_secret_draft.trim().to_string();
    if client_id.is_empty() || client_secret.is_empty() {
        state.auth =
            AuthState::Failed("Enter both the OAuth client id and the client secret.".to_string());
        return vec![];
    }
    state.session_generation += 1;
    state.auth = AuthState::Verifying;
    state.sign_in.oauth_url = None;
    vec![Effect::Api(ApiRequest::StartOAuth {
        client_id,
        client_secret,
    })]
}

/// Back to the sign-in page with a fresh state. Playback stops and
/// the queue empties: nothing of the session stays audible. Only the
/// volume and equalizer settings survive.
pub(super) fn sign_out(state: &mut State) -> Vec<Effect> {
    let equalizer = state.equalizer.clone();
    let volume = state.playback.volume;
    let balance = state.playback.balance;
    let generation = state.session_generation + 1;
    *state = State::default();
    state.session_generation = generation;
    state.playback.volume = volume;
    state.playback.balance = balance;
    state.equalizer = equalizer;
    vec![
        Effect::Player(PlayerCommand::Stop),
        Effect::ClearCredentials,
        Effect::ClearLibraryCache,
    ]
}

pub(super) fn finish_sign_in(
    state: &mut State,
    result: Result<(), crate::core::sign_in::SignInFailure>,
) -> Vec<Effect> {
    match result {
        Ok(()) => {
            state.auth = AuthState::SignedIn;
            state.sign_in.saved_account = true;
            state.sign_in.oauth_url = None;
            navigate(state, Page::Home)
        }
        Err(message) => {
            state.sign_in.oauth_url = None;
            state.auth = match message {
                crate::core::sign_in::SignInFailure::Connection => AuthState::ConnectionFailed,
                crate::core::sign_in::SignInFailure::Expired => AuthState::Expired,
                crate::core::sign_in::SignInFailure::Configuration => {
                    AuthState::Failed(message.message().into())
                }
            };
            vec![]
        }
    }
}
