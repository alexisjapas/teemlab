//! The **Experiment** — the *saved parameters* of a Lab run (redesign Phase B,
//! `docs/ui-redesign.md` §6): which scenario, the search config (breeding params
//! and/or a parameter **sweep**), and the seed. It is the **MVP persistence unit** —
//! distinct from a *Run record* (the time series a run produced, deferred with
//! Analyze). Data + IO only; [`crate::panels`] renders the Lab setup that authors it.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::path::Path;
use teemlab::config::ScenarioError;

/// Directory for user-saved experiments (gitignored, like `scenarios/saved/`).
pub const EXPERIMENTS_DIR: &str = "experiments/saved";

/// Which search the Lab runs (they nest — §6): pure **breeding**, a pure **sweep**, or
/// a sweep whose every value runs a breeding (the 2D search).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum LabMode {
    /// The generational `run → score → breed` loop (the dashboard).
    #[default]
    Breed,
    /// A seed / parameter sweep, scoring each world.
    Sweep,
    /// An outer sweep whose every value runs an inner breeding.
    Both,
}

impl LabMode {
    /// Whether this mode includes a parameter sweep.
    pub fn has_sweep(self) -> bool {
        matches!(self, LabMode::Sweep | LabMode::Both)
    }
    /// Whether this mode includes a breeding.
    pub fn has_breed(self) -> bool {
        matches!(self, LabMode::Breed | LabMode::Both)
    }
}

/// A **sweep** over one scenario parameter — the Lab's outer search axis (§6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SweepParams {
    /// Which parameter to sweep — a label the `sweep` bin maps to a config field.
    pub parameter: String,
    /// Range start.
    pub min: f32,
    /// Range end.
    pub max: f32,
    /// Number of steps (inclusive of the ends).
    pub steps: u32,
}

impl Default for SweepParams {
    fn default() -> Self {
        Self {
            parameter: "nutrient decay".to_string(),
            min: 0.0,
            max: 0.1,
            steps: 5,
        }
    }
}

/// The saved parameters of a Lab manipulation — the persistence unit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Experiment {
    /// Display name (the file stem).
    pub name: String,
    /// The scenario it ran against (its origin label / file stem).
    pub scenario: String,
    /// Base RNG seed.
    pub seed: u64,
    /// The search mode.
    pub mode: LabMode,
    /// The sweep configuration (meaningful when the mode sweeps).
    pub sweep: SweepParams,
    /// Free notes.
    pub notes: String,
}

impl Experiment {
    /// Serialize to pretty RON.
    pub fn to_ron_string(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Write to a RON file, creating the parent directory.
    pub fn save_ron_file(&self, path: impl AsRef<Path>) -> Result<(), ScenarioError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = self
            .to_ron_string()
            .map_err(|e| ScenarioError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// The Lab screen's in-progress **setup** state (the experiment being configured) —
/// the search mode, the sweep params, the experiment name buffer, and whether to
/// persist a run record (default **on** for the Lab, ui-redesign §6).
#[derive(Resource)]
pub struct LabSetup {
    /// The search mode.
    pub mode: LabMode,
    /// The sweep configuration.
    pub sweep: SweepParams,
    /// Name buffer for "Save Experiment".
    pub experiment_name: String,
    /// Persist this run's full metrics for Analyze — Lab default **on** (a search is
    /// worth analysing). Inert until run records land (B8).
    pub run_record: bool,
}

impl Default for LabSetup {
    fn default() -> Self {
        Self {
            mode: LabMode::default(),
            sweep: SweepParams::default(),
            experiment_name: String::new(),
            run_record: true,
        }
    }
}

impl LabSetup {
    /// Snapshot the current setup as a savable [`Experiment`] against `scenario`.
    pub fn to_experiment(&self, scenario: String, seed: u64) -> Experiment {
        Experiment {
            name: self.experiment_name.trim().to_string(),
            scenario,
            seed,
            mode: self.mode,
            sweep: self.sweep.clone(),
            notes: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_flags() {
        assert!(LabMode::Breed.has_breed() && !LabMode::Breed.has_sweep());
        assert!(LabMode::Sweep.has_sweep() && !LabMode::Sweep.has_breed());
        assert!(LabMode::Both.has_sweep() && LabMode::Both.has_breed());
    }

    #[test]
    fn experiment_ron_round_trips() {
        let setup = LabSetup {
            mode: LabMode::Both,
            experiment_name: "decay ladder".to_string(),
            ..LabSetup::default()
        };
        let exp = setup.to_experiment("reef".to_string(), 12648430);
        let text = exp.to_ron_string().expect("serialize");
        let back: Experiment = ron::from_str(&text).expect("parse");
        assert_eq!(exp, back);
        assert_eq!(back.name, "decay ladder");
        assert_eq!(back.scenario, "reef");
    }
}
