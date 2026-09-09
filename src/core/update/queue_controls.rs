use super::{Action, Effect, RandomBelow, State, load_or_stop, prefetch_next};

pub(super) fn apply(state: &mut State, action: Action, random_below: RandomBelow) -> Vec<Effect> {
    match action {
        Action::QueueRemoved(index) => {
            state.playback.queue.remove_upcoming(index);
            prefetch_next(state)
        }
        Action::TrackPlayNext(track) => {
            state.playback.queue.play_next(track);
            prefetch_next(state)
        }
        Action::TrackQueued(track) => {
            state.playback.queue.queue_track(track);
            prefetch_next(state)
        }
        Action::QueueJumped(index) => {
            if index >= state.playback.queue.upcoming().count() {
                return vec![];
            }
            load_or_stop(state, |state| state.playback.queue.jump_to(index))
        }
        Action::QueueSelectionRemoved(mut indices) => {
            indices.sort_unstable();
            indices.dedup();
            for index in indices.into_iter().rev() {
                state.playback.queue.remove_upcoming(index);
            }
            prefetch_next(state)
        }
        Action::QueueSelectionMoved { selected, before } => {
            state.playback.queue.move_upcoming(&selected, before);
            prefetch_next(state)
        }
        Action::QueueCleared => {
            state.playback.queue.clear_user_queue();
            prefetch_next(state)
        }
        Action::ShuffleToggled => {
            let on = !state.playback.queue.shuffle;
            state.playback.queue.set_shuffle(on, random_below);
            prefetch_next(state)
        }
        Action::RepeatCycled => {
            state.playback.queue.repeat = state.playback.queue.repeat.cycled();
            prefetch_next(state)
        }
        Action::AutoplayToggled => {
            state.playback.autoplay = !state.playback.autoplay;
            vec![]
        }
        Action::QueuePanelToggled => {
            state.queue_open = !state.queue_open;
            vec![]
        }
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}
