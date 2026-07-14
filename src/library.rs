//! The **Library** catalog model (redesign Phase B, `docs/ui-redesign.md` §4): the
//! browsable collections of **Worlds** and **Species**, and the **compose tray** — a
//! chosen World plus a cast of Species with counts — from which a runnable Scenario is
//! assembled and launched into Observe / Studio / Lab.
//!
//! Data + IO only (no egui); [`crate::panels`] renders it. Worlds in the *examples*
//! library are **derived** from the committed scenarios (deduplicated by content — many
//! scenarios share one stage), so the gallery is useful without any committed world
//! files; *saved* worlds come from `worlds/saved/`. Composition is **drop-only**
//! ([`compose`](Library::compose)): dropping a species into a world needs no relation
//! wiring — trophic interactions are emergent (Phase A) — and the food-web validator
//! ([`crate::trophic`]) tells the user whether the result is viable before it runs.

use bevy::prelude::Resource;
use std::path::PathBuf;
use teemlab::SimConfig;
use teemlab::config::{SpeciesEntry, World};

use crate::files::ron_files;

/// Which library a catalog entry belongs to — committed *examples* vs the user's *saved*.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CatalogSource {
    /// Committed, curated (`*/examples/`). Read-only (guardrailed against overwrite).
    #[default]
    Examples,
    /// User-saved (`*/saved/`, gitignored). Editable / deletable.
    Saved,
}

/// Which gallery the Library shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LibraryTab {
    /// The Worlds gallery.
    #[default]
    Worlds,
    /// The Species gallery.
    Species,
}

/// A browsable **World** in the catalog.
pub struct WorldEntry {
    /// Display name (a derived world takes its first scenario's stem).
    pub name: String,
    /// The abiotic stage.
    pub world: World,
    /// Which library it came from.
    pub source: CatalogSource,
    /// Path on disk for a *saved* world (`None` for one derived from a scenario).
    pub path: Option<PathBuf>,
    /// For a derived world, how many example scenarios share this exact stage.
    pub derived_from: usize,
}

/// A browsable **Species** in the catalog.
pub struct SpeciesCatalogEntry {
    /// Display name (the archetype's name).
    pub name: String,
    /// The library entry (body + brain + genes, possibly captured weights).
    pub entry: SpeciesEntry,
    /// Which library it came from.
    pub source: CatalogSource,
    /// Path on disk.
    pub path: PathBuf,
}

/// A cast member being composed: a species and how many to spawn.
pub struct ComposeItem {
    /// The species.
    pub entry: SpeciesEntry,
    /// Spawn count (matches [`teemlab::config::Archetype::count`]).
    pub count: usize,
}

/// The Library's state — the catalogs, the browse filters, and the in-progress
/// composition (a World + a cast).
#[derive(Resource, Default)]
pub struct Library {
    /// The Worlds catalog (examples + saved).
    pub worlds: Vec<WorldEntry>,
    /// The Species catalog (examples + saved).
    pub species: Vec<SpeciesCatalogEntry>,
    /// Which gallery is shown.
    pub tab: LibraryTab,
    /// Which library (examples / saved) the gallery filters to.
    pub source: CatalogSource,
    /// Name / tag search filter.
    pub search: String,
    /// Index into [`worlds`](Self::worlds) of the chosen stage (the tray's World).
    pub chosen_world: Option<usize>,
    /// The cast being composed.
    pub cast: Vec<ComposeItem>,
    /// Scanned once, on the first Library visit (re-scan via [`reload`](Self::reload)).
    pub loaded: bool,
}

const WORLDS_SAVED_DIR: &str = "worlds/saved";
const SPECIES_EXAMPLES_DIR: &str = "species/examples";
const SPECIES_SAVED_DIR: &str = "species/saved";
const SCENARIOS_EXAMPLES_DIR: &str = "scenarios/examples";

impl Library {
    /// (Re)scan the catalogs from disk. Never errors — an unreadable / absent directory
    /// simply contributes nothing (a fresh checkout has no `saved/`).
    pub fn reload(&mut self) {
        self.worlds.clear();
        self.species.clear();

        // Example worlds: extract the stage from each committed scenario, deduplicated
        // by content (many scenarios share one world → one card, with a count).
        for path in ron_files(SCENARIOS_EXAMPLES_DIR) {
            let Ok(cfg) = SimConfig::from_ron_file(&path) else {
                continue;
            };
            let world = World::extract(&cfg);
            if let Some(existing) = self.worlds.iter_mut().find(|w| w.world == world) {
                existing.derived_from += 1;
            } else {
                self.worlds.push(WorldEntry {
                    name: stem(&path),
                    world,
                    source: CatalogSource::Examples,
                    path: None,
                    derived_from: 1,
                });
            }
        }
        // Saved worlds (authored in Studio / saved from the tray).
        for path in ron_files(WORLDS_SAVED_DIR) {
            if let Ok(world) = World::from_ron_file(&path) {
                self.worlds.push(WorldEntry {
                    name: stem(&path),
                    world,
                    source: CatalogSource::Saved,
                    path: Some(PathBuf::from(&path)),
                    derived_from: 0,
                });
            }
        }
        // Species.
        for (dir, source) in [
            (SPECIES_EXAMPLES_DIR, CatalogSource::Examples),
            (SPECIES_SAVED_DIR, CatalogSource::Saved),
        ] {
            for path in ron_files(dir) {
                if let Ok(entry) = SpeciesEntry::from_ron_file(&path) {
                    self.species.push(SpeciesCatalogEntry {
                        name: entry.archetype.name.clone(),
                        entry,
                        source,
                        path: PathBuf::from(&path),
                    });
                }
            }
        }
        self.loaded = true;
    }

    /// Compose the chosen World + cast into a runnable scenario. `None` if no World is
    /// chosen. **Drop-only**: species are added as archetypes with their counts, no
    /// relation wiring (interactions are emergent). Their nutritional profile is set in
    /// Studio — it does not yet travel with a library species (a follow-up).
    pub fn compose(&self) -> Option<SimConfig> {
        let world = &self.worlds.get(self.chosen_world?)?.world;
        let mut cfg = world.into_scenario();
        for item in &self.cast {
            let mut arch = item.entry.archetype.clone();
            arch.count = item.count;
            cfg.archetypes.push(arch);
        }
        Some(cfg)
    }

    /// Add a species to the compose cast (default count 1); if already present by name,
    /// bump its count instead of duplicating the row.
    pub fn add_to_cast(&mut self, entry: SpeciesEntry) {
        if let Some(item) = self
            .cast
            .iter_mut()
            .find(|c| c.entry.archetype.name == entry.archetype.name)
        {
            item.count += 1;
        } else {
            self.cast.push(ComposeItem { entry, count: 1 });
        }
    }
}

/// File stem of a path string (`scenarios/examples/20_reef.ron` → `20_reef`).
fn stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use teemlab::config::Archetype;

    fn species(name: &str) -> SpeciesEntry {
        let mut a = Archetype::new_agent(0);
        a.name = name.to_string();
        SpeciesEntry::base(a)
    }

    #[test]
    fn compose_needs_a_world() {
        let lib = Library::default();
        assert!(lib.compose().is_none(), "no chosen World → nothing to run");
    }

    #[test]
    fn compose_drops_the_cast_into_the_world_with_counts() {
        let mut lib = Library::default();
        lib.worlds.push(WorldEntry {
            name: "reef".into(),
            world: World::default(),
            source: CatalogSource::Examples,
            path: None,
            derived_from: 1,
        });
        lib.chosen_world = Some(0);
        lib.add_to_cast(species("Grazer"));
        lib.add_to_cast(species("Grazer")); // same name → bump, not duplicate
        lib.add_to_cast(species("Kelp"));

        assert_eq!(lib.cast.len(), 2, "same-named species bump the count");
        let cfg = lib.compose().expect("a World is chosen");
        assert_eq!(
            cfg.archetypes.len(),
            2,
            "the two distinct species are dropped in"
        );
        let grazer = cfg.archetypes.iter().find(|a| a.name == "Grazer").unwrap();
        assert_eq!(
            grazer.count, 2,
            "the bumped count carries into the scenario"
        );
    }
}
