//! Unified UI status line of the windowed build.
//!
//! A module of the windowed *binary* only. Every transient UI feedback — scenario
//! save/load/reload, species import/save, archetype capture, video recording — funnels
//! into this single resource, shown **once** in the bottom bar, instead of three
//! separate per-panel strings rendered in three different places.
//!
//! Each message carries a **kind** (info / success / error), which colours it and sets
//! its lifetime: info and success **expire** after a few seconds so a stale "Saved →"
//! doesn't haunt the session, while an error **persists** until something replaces it.
//! `panels::dock` stamps a freshly-set message with the current time and renders it only
//! while [`UiStatus::visible`]. No simulation logic: pure presentation state.

use bevy::prelude::Resource;

/// How long an info/success message stays visible (seconds).
const EXPIRY: f64 = 8.0;

/// The nature of a status message — drives its colour and lifetime.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum StatusKind {
    /// A neutral note (default) — expires.
    #[default]
    Info,
    /// A completed action (a save) — expires.
    Success,
    /// A failure — persists until replaced.
    Error,
}

/// The single status message shown in the bottom bar (empty = nothing to report).
#[derive(Resource, Default)]
pub struct UiStatus {
    pub message: String,
    pub kind: StatusKind,
    /// When the message was stamped (seconds, `Time<Real>`). `NEG_INFINITY` marks a
    /// freshly-set, not-yet-stamped message (visible until `dock` stamps it next frame).
    set_at: f64,
}

impl UiStatus {
    /// Replace the status with a neutral **info** message.
    pub fn set(&mut self, message: impl Into<String>) {
        self.set_kind(StatusKind::Info, message);
    }

    /// Replace the status with a **success** message (a completed save, etc.).
    pub fn ok(&mut self, message: impl Into<String>) {
        self.set_kind(StatusKind::Success, message);
    }

    /// Replace the status with an **error** message (persists until replaced).
    pub fn error(&mut self, message: impl Into<String>) {
        self.set_kind(StatusKind::Error, message);
    }

    /// Set from a helper that returns a human message where a **failure contains
    /// "failed"** (the convention of `editor::save_variant` / `export_species` /
    /// `import_species`): success otherwise. Lets those String-returning helpers colour
    /// their outcome without each learning about [`StatusKind`].
    pub fn set_result(&mut self, message: impl Into<String>) {
        let m = message.into();
        let kind = if m.to_ascii_lowercase().contains("failed") {
            StatusKind::Error
        } else {
            StatusKind::Success
        };
        self.set_kind(kind, m);
    }

    fn set_kind(&mut self, kind: StatusKind, message: impl Into<String>) {
        self.message = message.into();
        self.kind = kind;
        self.set_at = f64::NEG_INFINITY; // stamped by `dock` next frame
    }

    /// Stamp a freshly-set message with `now` so its expiry can be measured. Idempotent:
    /// already-stamped messages keep their timestamp. Called once per frame by `dock`.
    pub fn stamp(&mut self, now: f64) {
        if self.set_at.is_sign_negative() && self.set_at.is_infinite() {
            self.set_at = now;
        }
    }

    /// Whether the message should be shown at `now`: errors always (while non-empty),
    /// info/success until [`EXPIRY`] has elapsed since it was stamped (an unstamped
    /// message — set this frame — always shows).
    pub fn visible(&self, now: f64) -> bool {
        if self.message.is_empty() {
            return false;
        }
        match self.kind {
            StatusKind::Error => true,
            _ => !self.set_at.is_finite() || now - self.set_at < EXPIRY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_info_expires_error_persists() {
        let mut s = UiStatus::default();
        s.set("saved");
        s.stamp(0.0);
        assert!(s.visible(7.9), "info visible before expiry");
        assert!(!s.visible(8.1), "info gone after expiry");

        s.error("boom");
        s.stamp(0.0);
        assert!(s.visible(60.0), "error persists well past expiry");
    }

    #[test]
    fn set_replaces_kind_and_timestamp() {
        let mut s = UiStatus::default();
        s.ok("done");
        s.stamp(0.0);
        assert_eq!(s.kind, StatusKind::Success);
        // A new message resets the kind and un-stamps it (visible again until re-stamped).
        s.error("failed");
        assert_eq!(s.kind, StatusKind::Error);
        assert!(s.visible(100.0), "freshly set, not yet stamped → visible");
    }
}
