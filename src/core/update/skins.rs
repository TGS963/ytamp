use super::{Action, Effect, State};

pub(super) fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::WinampToggled => toggle_winamp(state),
        Action::WinampScaleSet(scale) => {
            state.winamp.scale = scale.clamp(1, 4);
            vec![]
        }
        Action::WinampOnTopToggled => {
            state.winamp.on_top = !state.winamp.on_top;
            vec![]
        }
        Action::SkinBrowserToggled => {
            state.skin_browser_open = !state.skin_browser_open;
            vec![]
        }
        Action::SkinChosen(skin) => wear_skin(state, skin),
        Action::SkinFileDropped(path) => vec![Effect::InstallSkin(path)],
        Action::SkinLoaded(_) => vec![],
        Action::SkinInstalled(Ok(stem)) => wear_skin(state, Some(stem)),
        Action::SkinInstalled(Err(message)) => {
            state.notices.push(message);
            vec![]
        }
        Action::SkinListRefreshed(names) => {
            state.winamp.available_skins = names;
            vec![]
        }
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}

/// Opens or closes the Winamp skin window. Opening it loads the worn
/// skin and refreshes the skins folder listing, so both are fresh
/// whether this is the first open or a later one.
pub(super) fn toggle_winamp(state: &mut State) -> Vec<Effect> {
    state.winamp.open = !state.winamp.open;
    if !state.winamp.open {
        return vec![];
    }
    vec![
        Effect::LoadSkin(state.winamp.skin.clone()),
        Effect::RefreshSkinList,
    ]
}

/// Wears a skin: records the choice and loads it. `SkinChosen` and a
/// successful `SkinInstalled` both end up here, since installing a
/// skin means wearing it.
pub(super) fn wear_skin(state: &mut State, skin: Option<String>) -> Vec<Effect> {
    state.winamp.skin = skin.clone();
    vec![Effect::LoadSkin(skin)]
}
