# teemlab

Moteur de simulation évolutive. **Un seul moteur** qui interprète de la donnée ;
chaque simulation (sélection naturelle, bataille, …) n'est qu'un *scénario*. Vue
2D top-down, entités = ronds. Boucle unique : **percevoir → décider → agir**.

## État : P0 — Fondations

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
  config.rs       SimConfig : embryon du futur fichier de scénario.
  components.rs    Corps de l'agent ; Perception/Action = contrat du cerveau.
  brain.rs        Brain (enum, dispatch statique) ; WanderBrain déterministe.
  movement.rs     Systèmes percevoir / décider / agir (FixedUpdate, chaînés).
  rng.rs          PRNG déterministe minimal (SplitMix64), par agent.
  spawn.rs        Peuplement : arène (murs statiques) + agents.
  main.rs         Binaire fenêtré  → `teemlab`.
  bin/headless.rs Binaire headless → `headless`.
```

## Développement

L'environnement (toolchain Rust + dépendances système de Bevy) est fourni par
Nix :

```sh
nix develop            # ou : direnv allow  (puis automatique)
cargo run --bin teemlab     # fenêtré
cargo run --bin headless    # headless (stepping à nombre de ticks fixe)
```
