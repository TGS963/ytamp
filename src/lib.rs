//! ytamp: YouTube Music, native and fast.
//!
//! The library exists so examples and integration tests can drive the
//! same modules the binary runs.

pub mod api;
pub mod app;
pub mod auth;
pub mod branding;
pub mod core;
pub mod fonts;
pub mod library_cache;
pub mod media_keys;
pub mod player;
pub mod runtime;
pub mod skin;
pub mod skins_dir;
pub mod stream;
pub mod theme;
pub mod thumbnails;
pub mod ui;
pub mod vis;

pub mod menu_bar;

mod rustypipe_client;
