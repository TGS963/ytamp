use super::{Action, EffectRuntime};

impl EffectRuntime {
    pub(super) fn cancel_lyrics(&self) {
        if let Some(task) = self.pending_lyrics.lock().expect("lyrics task lock").take() {
            task.abort();
        }
    }

    pub(super) fn fetch_lyrics(&self, request: Option<(u64, crate::core::model::TrackId)>) {
        self.cancel_lyrics();
        if let Some((request_id, track)) = request {
            let deliver = self.delivery();
            *self.pending_lyrics.lock().expect("lyrics task lock") = Some(
                self.tokio
                    .spawn(async move {
                        let result = tokio::time::timeout(
                            std::time::Duration::from_secs(20),
                            crate::api::lyrics::fetch(&track),
                        )
                        .await
                        .unwrap_or_else(|_| Err("The lyrics request timed out. Try again.".into()));
                        deliver(Action::LyricsLoaded {
                            request_id,
                            track,
                            result,
                        });
                    })
                    .abort_handle(),
            );
        }
    }
}
