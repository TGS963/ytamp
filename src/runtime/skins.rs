use super::{Action, EffectRuntime};
use crate::{skin::Skin, skins_dir};
use std::{path::PathBuf, sync::Arc};

impl EffectRuntime {
    /// Decodes a skin on the blocking pool and delivers it. `App`
    /// routes the result straight to the shell, never to the reducer.
    pub(super) fn load_skin(&self, name: Option<String>) {
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            deliver(Action::SkinLoaded(load_named_skin(name)));
        });
    }

    /// Copies a dropped skin into the skins folder on the blocking
    /// pool and delivers the result.
    pub(super) fn install_skin(&self, path: PathBuf) {
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            deliver(Action::SkinInstalled(skins_dir::install(&path)));
        });
    }

    /// Lists the skins folder again on the blocking pool.
    pub(super) fn refresh_skin_list(&self) {
        let deliver = self.delivery();
        self.tokio
            .spawn_blocking(move || deliver(Action::SkinListRefreshed(skins_dir::list_skins())));
    }
}

/// The built-in skin for `None`, or the named skin from the skins
/// folder. A skin that has gone missing since it was listed, or that
/// fails to decode, is reported as an error rather than silently
/// falling back, so the listener learns their skin is gone.
pub(super) fn load_named_skin(name: Option<String>) -> Result<Arc<Skin>, String> {
    let Some(name) = name else {
        return Ok(Skin::builtin());
    };
    let path = skins_dir::skin_path(&name)
        .ok_or_else(|| format!("the skin \"{name}\" is no longer in the skins folder"))?;
    Skin::load(&path)
        .map(Arc::new)
        .map_err(|error| error.to_string())
}
