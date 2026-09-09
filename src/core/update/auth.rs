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
    let winamp = state.winamp.clone();
    let playback_generation = state.playback_generation;
    let mut playback = state.playback.clone();
    let had_remote_current = playback
        .queue
        .current()
        .is_some_and(|track| !track.is_local());
    playback.queue.retain_local();
    let pending_import = state.imports.pending();
    let local_mode = pending_import
        || playback
            .queue
            .current()
            .is_some_and(|track| track.is_local())
        || playback.queue.upcoming().any(|track| track.is_local());
    let local_current = playback
        .queue
        .current()
        .is_some_and(|track| track.is_local());
    let imports = std::mem::take(&mut state.imports);
    playback.last_hover_prefetch = None;
    playback.radio_request = None;
    playback.duration_lookup = Default::default();
    let local_next = local_current.then(|| playback.queue.peek_next());
    if had_remote_current {
        playback.error = None;
        playback.loading = false;
        playback.status = super::PlayStatus::Stopped;
        playback.position = std::time::Duration::ZERO;
        playback.track_duration = None;
        playback.resume_position = None;
        playback.channels = 0;
        playback.sample_rate = 0;
    }
    let generation = state.session_generation + 1;
    *state = State::default();
    state.session_generation = generation;
    state.playback_generation = playback_generation;
    state.playback = playback;
    state.equalizer = equalizer;
    state.winamp = winamp;
    state.local_mode = local_mode;
    state.imports = imports;
    let mut effects = vec![Effect::ClearCredentials, Effect::ClearLibraryCache];
    if had_remote_current {
        effects.insert(0, Effect::Player(PlayerCommand::Stop));
    } else if local_current {
        effects.push(Effect::Player(PlayerCommand::PrepareNext(
            local_next.flatten(),
        )));
    }
    effects
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
