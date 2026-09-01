//! The functional core: state, actions, effects, and the reducer.
//!
//! Nothing in this module touches the network, the audio device, the
//! clock, or the filesystem.

pub mod action;
pub mod effect;
pub mod model;
pub mod queue;
pub mod session;
pub mod state;
pub mod update;
