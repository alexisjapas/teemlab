//! Unified UI status line of the windowed build.
//!
//! A module of the windowed *binary* only. Every transient UI feedback — scenario
//! save/load/reload, species import/export/sync, archetype capture, video recording
//! — funnels into this single resource, shown **once** in the bottom bar, instead of
//! three separate per-panel strings rendered in three different places.
//!
//! No simulation logic: pure presentation state.

use bevy::prelude::Resource;

/// The single status message shown in the bottom bar (empty = nothing to report).
#[derive(Resource, Default)]
pub struct UiStatus {
    pub message: String,
}

impl UiStatus {
    /// Replace the current status message.
    pub fn set(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }
}
