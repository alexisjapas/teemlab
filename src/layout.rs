//! Windowed-UI **layout constants**: the fixed widths/heights of Observe's docked
//! regions.
//!
//! [`panels::dock`]: crate::panels::dock
//!
//! Observe's panels are **fixed** — not resizable, not foldable (a redesign decision:
//! a panel dragged wide or folded away only fights the arena, which is the observation
//! lead). Each region therefore has one size, defined here so [`panels::dock`] stays a
//! thin caller. (Studio keeps its own resizable editor columns — a different screen.)

/// The **live-stats** panel's width (Observe, left): the comp's 216 px stat column.
pub const STATS_DEFAULT: f32 = 216.0;

/// The **agent-inspector** panel's width (Observe, right): the comp's 312 px column.
pub const INSPECTOR_DEFAULT: f32 = 312.0;

/// The **curves strip**'s height (Observe, bottom): tall enough that the two plots
/// (population + gene drift) read clearly — [`crate::hud::hud_section`] gives each plot
/// roughly this height minus its header/caption/legend chrome.
pub const BOTTOM_DEFAULT: f32 = 240.0;
