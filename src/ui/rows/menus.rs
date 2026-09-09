use super::RowContext;
use crate::core::action::Action;
use crate::core::model::Track;
use crate::core::state::Page;
use crate::core::update::is_liked;

/// The row's right-click menu: like or unlike, add to the queue, add
/// to a playlist, and, on a playlist page, remove from it. Returns at
/// most one action, the last menu item the user clicked.
pub(crate) fn row_context_menu(
    response: &egui::Response,
    track: &Track,
    context: &RowContext,
) -> Option<Action> {
    let mut action = None;
    let secondary = response.contains_pointer()
        && response
            .ctx
            .input(|input| input.pointer.secondary_clicked());
    let command = if secondary {
        Some(egui::SetOpenCommand::Bool(true))
    } else if response.clicked() {
        Some(egui::SetOpenCommand::Bool(false))
    } else {
        None
    };
    egui::Popup::context_menu(response)
        .open_memory(command)
        .show(|ui| {
            like_menu_item(ui, track, context, &mut action);
            if ui.button("Play next").clicked() {
                action = Some(Action::TrackPlayNext(track.clone()));
                ui.close();
            }
            if ui.button("Start radio").clicked() {
                action = Some(Action::RadioStartRequested(track.clone()));
                ui.close();
            }
            if ui.button("Add to queue").clicked() {
                action = Some(Action::TrackQueued(track.clone()));
                ui.close();
            }
            add_to_playlist_menu(ui, track, context, &mut action);
            remove_from_playlist_menu_item(ui, track, context, &mut action);
        });
    action
}

fn like_menu_item(
    ui: &mut egui::Ui,
    track: &Track,
    context: &RowContext,
    action: &mut Option<Action>,
) {
    let label = if is_liked(context.liked, &track.id) {
        "Unlike"
    } else {
        "Like"
    };
    if ui.button(label).clicked() {
        *action = Some(Action::TrackLikeToggled(track.clone()));
        ui.close();
    }
}

/// The "Add to playlist" submenu: one entry per playlist in the
/// library, plus "New playlist..." to create one and add the track to
/// it once it exists.
fn add_to_playlist_menu(
    ui: &mut egui::Ui,
    track: &Track,
    context: &RowContext,
    action: &mut Option<Action>,
) {
    ui.menu_button("Add to playlist", |ui| {
        let max_height = (ui.ctx().content_rect().height() - 100.0).clamp(80.0, 320.0);
        ui.set_max_width(280.0);
        egui::ScrollArea::vertical()
            .id_salt("playlist-targets")
            .max_height(max_height)
            .show(ui, |ui| {
                for playlist in context.playlists {
                    if ui
                        .add(egui::Button::new(&playlist.title).truncate())
                        .clicked()
                    {
                        *action = Some(Action::TrackAddedToPlaylist {
                            playlist: playlist.id.clone(),
                            track: track.clone(),
                        });
                        ui.close();
                    }
                }
            });
        ui.separator();
        if ui.button("New playlist...").clicked() {
            *action = Some(Action::CreatePlaylistDialogOpened(Some(track.clone())));
            ui.close();
        }
    });
}

/// "Remove from this playlist", shown only on a playlist page and only
/// for a row that carries the item id a removal needs.
fn remove_from_playlist_menu_item(
    ui: &mut egui::Ui,
    track: &Track,
    context: &RowContext,
    action: &mut Option<Action>,
) {
    let Page::Playlist(playlist_id) = context.page else {
        return;
    };
    let Some(item_id) = &track.playlist_item_id else {
        return;
    };
    if ui.button("Remove from this playlist").clicked() {
        *action = Some(Action::TrackRemovedFromPlaylist {
            playlist: playlist_id.clone(),
            item_id: item_id.clone(),
        });
        ui.close();
    }
}
