//! The functional core: state, actions, effects, and the reducer.
//!
//! Nothing in this module touches the network, the audio device, the
//! clock, or the filesystem.

pub mod action;
pub mod durations;
pub mod effect;
pub mod equalizer;
pub mod model;
pub mod queue;
pub mod session;
pub mod sign_in;
pub mod state;
pub mod update;

pub mod lyrics;

pub mod listening_history;

pub mod discovery;
