use super::{Action, EffectRuntime};
use crate::core::model::TrackId;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

impl EffectRuntime {
    pub(super) fn cancel_local_import(&self) {
        if let Some(cancel) = self.pending_import.lock().expect("import lock").take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    pub(super) fn import_local_files(&self, id: u64, paths: Vec<std::path::PathBuf>) {
        self.cancel_local_import();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.pending_import.lock().expect("import lock") = Some(cancel.clone());
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            let result = crate::local_media::import(&paths, &cancel, |done| {
                deliver(Action::LocalImportProgress { id, done });
            });
            if !cancel.load(Ordering::Relaxed) {
                let errors = result
                    .failures
                    .into_iter()
                    .map(|(path, error)| {
                        format!(
                            "{}: {error}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )
                    })
                    .collect();
                deliver(Action::LocalImportFinished {
                    id,
                    tracks: result.tracks,
                    errors,
                });
            }
        });
    }
    pub(super) fn pick_local_files(&self, generation: u64, replace: Option<TrackId>) {
        let deliver = self.delivery();
        self.tokio.spawn(async move {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title("Add audio or video files")
                .add_filter(
                    "Media",
                    &[
                        "mp3", "wav", "flac", "ogg", "m4a", "mp4", "mov", "mkv", "webm", "aif",
                        "aiff",
                    ],
                );
            let paths = if replace.is_some() {
                dialog
                    .pick_file()
                    .await
                    .into_iter()
                    .map(|file| file.path().to_path_buf())
                    .collect()
            } else {
                dialog
                    .pick_files()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|file| file.path().to_path_buf())
                    .collect()
            };
            deliver(Action::LocalFilesChosen {
                generation,
                replace,
                paths,
            });
        });
    }
    pub(super) fn fetch_local_lyrics(
        &self,
        request_id: u64,
        track: TrackId,
        path: std::path::PathBuf,
    ) {
        self.cancel_lyrics();
        let deliver = self.delivery();
        let task = self.tokio.spawn_blocking(move || {
            let result = crate::local_media::local_lyrics(&path);
            deliver(Action::LyricsLoaded {
                request_id,
                track,
                result,
            });
        });
        *self.pending_lyrics.lock().expect("lyrics task lock") = Some(task.abort_handle());
    }
}

impl Drop for EffectRuntime {
    fn drop(&mut self) {
        self.cancel_local_import();
    }
}
