# teemlab

Moteur de simulation évolutive. **Un seul moteur** qui interprète de la donnée ;
chaque simulation (sélection naturelle, bataille, …) n'est qu'un *scénario*. Vue
2D top-down, entités = ronds. Boucle unique : **percevoir → décider → agir**.

## État

### P1 — Scénario = donnée

- [x] `SimConfig` chargé depuis un fichier **RON**, pas codé en dur. Un scénario
  est de la donnée : le *même* fichier pilote le build fenêtré et le headless.
- [x] Override partiel (`#[serde(default)]`) : un scénario ne mentionne que ce
  qu'il change ; champ inconnu rejeté (`deny_unknown_fields`).
- [x] Chargement partagé via `SimConfig::from_cli()` : 1ᵉʳ argument = chemin du
  scénario, absent → défauts, illisible/invalide → échec bruyant (sortie 1).

### P0 — Fondations

- [x] Bevy 0.18 + Avian 0.6, ronds rigides, collisions, caméra 2D top-down.
- [x] Boucle percevoir→décider→agir avec cerveau déterministe trivial (errance).
- [x] Deux points d'entrée partageant **le même** schedule de sim :
  - fenêtré (`DefaultPlugins`)
  - headless (`ScheduleRunnerPlugin`, stepping à nombre de ticks fixe).

**Invariant cardinal :** aucune logique de simulation dans `Update`. L'agentivité
vit dans `FixedUpdate`, la physique Avian dans `FixedPostUpdate`. `Update` est
réservé au rendu / UI du binaire fenêtré.

## Architecture

```
src/
  lib.rs          SimPlugin : le cœur render-agnostic partagé.
  config.rs       SimConfig : le scénario (RON) + son chargement.
  components.rs    Corps de l'agent ; Perception/Action = contrat du cerveau.
  brain.rs        Brain (enum, dispatch statique) ; WanderBrain déterministe.
  movement.rs     Systèmes percevoir / décider / agir (FixedUpdate, chaînés).
  rng.rs          PRNG déterministe minimal (SplitMix64), par agent.
  spawn.rs        Peuplement : arène (murs statiques) + agents.
  main.rs         Binaire fenêtré  → `teemlab`.
  bin/headless.rs Binaire headless → `headless`.
scenarios/
  default.ron     Scénario par défaut, tous champs documentés.
  crowded.ron     Variante (petite arène saturée) : override partiel.
```

## Développement

L'environnement (toolchain Rust + dépendances système de Bevy) est fourni par
Nix :

```sh
nix develop            # ou : direnv allow  (puis automatique)
cargo run --bin teemlab     # fenêtré, scénario par défaut
cargo run --bin headless    # headless, scénario par défaut

# Charger un scénario explicite (1ᵉʳ argument = chemin RON) :
cargo run --bin teemlab  scenarios/crowded.ron
cargo run --bin headless scenarios/default.ron
```
