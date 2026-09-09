//! Recovery decisions exposed to the UI, without raw server responses or secrets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInFailure {
    Connection,
    Expired,
    Configuration,
}
impl SignInFailure {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Connection => {
                "We couldn’t reach YouTube. Your saved sign-in is still here. Try again when your connection is ready."
            }
            Self::Expired => {
                "Google needs you to sign in again. Your queue and settings are still here."
            }
            Self::Configuration => {
                "Check your Google OAuth setup and enable YouTube Data API v3, then try again."
            }
        }
    }
    /// The upstream OAuth client wraps Google's machine-readable error in its
    /// parse error. Match only known auth/configuration codes; network errors
    /// must never be treated as a revoked login.
    pub fn classify(message: &str) -> Self {
        if message.contains("invalid_grant")
            || message.contains("invalid_token")
            || message.contains("401 Unauthorized")
        {
            Self::Expired
        } else if message.contains("invalid_client")
            || message.contains("unauthorized_client")
            || message.contains("accessNotConfigured")
            || message.contains("has not been used in project")
            || message.contains("insufficientPermissions")
        {
            Self::Configuration
        } else {
            Self::Connection
        }
    }
}
impl std::fmt::Display for SignInFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for SignInFailure {}
impl From<String> for SignInFailure {
    fn from(message: String) -> Self {
        Self::classify(&message)
    }
}
impl From<&str> for SignInFailure {
    fn from(message: &str) -> Self {
        Self::classify(message)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_failures_do_not_request_new_credentials() {
        for error in [
            "error sending request",
            "timed out",
            "YouTube answered 503",
            "quotaExceeded",
        ] {
            assert_eq!(SignInFailure::classify(error), SignInFailure::Connection);
        }
        assert_eq!(
            SignInFailure::classify("invalid_grant"),
            SignInFailure::Expired
        );
        assert_eq!(
            SignInFailure::classify("invalid_client"),
            SignInFailure::Configuration
        );
        assert_eq!(
            SignInFailure::classify("YouTube Data API has not been used in project"),
            SignInFailure::Configuration
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::core::{
        action::Action,
        effect::{AuthMethod, Effect},
        model::{Track, TrackId},
        state::{AuthState, PlayStatus, State},
        update::update,
    };
    fn stored() -> Action {
        Action::StoredAuthFound(AuthMethod::OAuthToken(
            r#"{"client_id":"test-client","client_secret":"test-secret"}"#.into(),
        ))
    }
    fn apply(state: &mut State, action: Action) -> Vec<Effect> {
        update(state, action, &mut |_| 0)
    }
    #[test]
    fn a_connection_retry_preserves_login_queue_and_position_and_rejects_stale_success() {
        let mut state = State::default();
        apply(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![Track {
                    source: Default::default(),
                    id: TrackId("song".into()),
                    title: "Song".into(),
                    artists: vec![],
                    album: None,
                    album_id: None,
                    duration: None,
                    thumbnail_url: None,
                    playlist_item_id: None,
                }],
                start: 0,
            },
        );
        state.playback.status = PlayStatus::Paused;
        state.playback.position = std::time::Duration::from_secs(33);
        apply(&mut state, stored());
        let generation = state.session_generation;
        apply(
            &mut state,
            Action::AuthVerified(Err(SignInFailure::Connection)),
        );
        assert_eq!(state.auth, AuthState::ConnectionFailed);
        assert!(state.sign_in.saved_account);
        assert_eq!(state.sign_in.client_id_draft, "test-client");
        assert_eq!(
            apply(&mut state, Action::SignInRetryRequested),
            vec![Effect::LoadStoredAuth]
        );
        assert!(apply(&mut state, Action::SignInRetryRequested).is_empty());
        apply(
            &mut state,
            Action::ForSession {
                generation,
                action: Box::new(Action::AuthVerified(Ok(()))),
            },
        );
        assert_eq!(state.auth, AuthState::Verifying);
        assert_eq!(state.playback.queue.current().unwrap().id.0, "song");
        assert_eq!(state.playback.position.as_secs(), 33);
        apply(&mut state, stored());
        apply(&mut state, Action::AuthVerified(Ok(())));
        assert_eq!(state.auth, AuthState::SignedIn);
    }
    #[test]
    fn revoked_access_and_connection_failure_have_different_recovery() {
        let mut state = State::default();
        apply(&mut state, stored());
        apply(
            &mut state,
            Action::AuthVerified(Err(SignInFailure::Expired)),
        );
        assert_eq!(state.auth, AuthState::Expired);
        assert_eq!(state.sign_in.client_secret_draft, "test-secret");
        assert!(!apply(&mut state, Action::OAuthStartRequested).is_empty());
        assert!(apply(&mut state, Action::OAuthStartRequested).is_empty());
        let generation = state.session_generation;
        apply(&mut state, Action::SignInCancelled);
        apply(
            &mut state,
            Action::ForSession {
                generation,
                action: Box::new(Action::OAuthTokenStored),
            },
        );
        assert_eq!(state.auth, AuthState::ConnectionFailed);
    }
    #[test]
    fn active_session_expiry_keeps_browse_and_library_state() {
        let mut state = State::default();
        apply(&mut state, Action::AuthVerified(Ok(())));
        let page = state.page.clone();
        apply(&mut state, Action::SessionExpired);
        assert_eq!(state.auth, AuthState::Expired);
        assert_eq!(state.page, page);
    }
}
