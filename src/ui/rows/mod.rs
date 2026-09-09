//! Shared list rows: the one way every view draws a clickable track list.

use crate::core::model::{Playlist, Track};
use crate::core::state::{Loadable, Page};

mod cells;
mod list;
mod menus;
#[cfg(test)]
mod tests;

#[cfg(test)]
use cells::{DwellDecision, EMITTED, dwell_decision};
pub(crate) use cells::{artist_labels, format_duration};
pub use list::{artwork, row_frame, track_list, track_list_capped};
pub(crate) use menus::row_context_menu;

pub struct RowContext<'a> {
    pub liked: &'a Loadable<Vec<Track>>,
    pub playlists: &'a [Playlist],
    pub page: &'a Page,
}
