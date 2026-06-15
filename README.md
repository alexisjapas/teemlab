# teemlab

Moteur de simulation évolutive. **Un seul moteur** qui interprète de la donnée ;
chaque simulation (sélection naturelle, bataille, …) n'est qu'un *scénario*. Vue
2D top-down, entités = ronds. Boucle unique : **percevoir → décider → agir**.

## État

Le plan complet et son avancement vivent dans [`ROADMAP.md`](ROADMAP.md) ; la
numérotation de phase (P0, P1, …) y fait foi. Résumé :

### P0 — Fondations — ✅ terminé

- [x] Bevy 0.18 + Avian 0.6, ronds rigides, collisions, caméra 2D top-down.
- [x] Boucle percevoir→décider→agir avec cerveau déterministe trivial (errance).
- [x] Deux points d'entrée partageant **le même** schedule de sim :
  - fenêtré (`DefaultPlugins`)
  - headless (`ScheduleRunnerPlugin`, stepping à nombre de ticks fixe).

### P1 — Le moteur jouable — ✅ terminé

La première **boucle évolutive continue** tourne de bout en bout, pilotable
depuis l'UI. **Scénario = donnée** :

- [x] `SimConfig` chargé depuis un fichier **RON**, pas codé en dur. Un scénario
  est de la donnée : le *même* fichier pilote le build fenêtré et le headless.
- [x] Override partiel (`#[serde(default)]`) : un scénario ne mentionne que ce
  qu'il change ; champ inconnu rejeté (`deny_unknown_fields`).
- [x] Chargement partagé via `SimConfig::from_cli()` : 1ᵉʳ argument = chemin du
  scénario, absent → défauts, illisible/invalide → échec bruyant (sortie 1).
- [x] **Vision par raycast avec occlusion** (spatial queries Avian) : chaque
  agent éventaille des rayons sur son champ de vision, ne garde que le hit le
  plus proche (un mur masque ce qui est derrière), et écrit une *proximité*
  normalisée par rayon dans `Perception`. Coût métabolique quantifié
  (`Vision::metabolic_cost`), prélèvement différé à l'économie d'énergie.
- [x] **Primitive d'interaction unique** (§3 *manger et attaquer sont le même
  verbe*) : un acteur réduit la `Reserve` d'une cible à portée (broad-phase
  Avian) ; `transfer: true` = prédation, `false` = combat. `Species` + table
  `relations` (RON) décident qui agit sur qui. Voir `scenarios/predation.ron`.
- [x] **Scénario nº1 — sélection naturelle** (`scenarios/selection.ron`) :
  économie d'énergie complète. Métabolisme (base + locomotion + coût de vision),
  nourriture mangée via l'unique primitive d'interaction, mort à zéro,
  réensemencement de la nourriture. Calibrée pour que le butinage soutienne la
  population (les affamés meurent).
- [x] **Reproduction + mutation** (`scenarios/evolution.ron`) : la boucle
  évolutive continue. `Genotype` héritable (vitesse, agilité, vision) **compilé**
  en phénotype au spawn (§2) ; un agent assez nourri engendre un enfant muté
  (gaussienne bornée). Repousse de nourriture à débit fini → capacité de charge.
  Sur 100 s : population stable, la portée de vision **dérive vers le bas** (pur
  coût tant que l'errance l'ignore), la vitesse monte (meilleur butinage).
- [x] **Placement manuel (drag-and-drop)** : panneau d'archétypes egui à droite
  (une entrée par espèce + nourriture), bandeau de stats en bas, dépose dans
  l'aire de jeu. Éditeur fenêtré uniquement (`src/editor.rs`).
- [x] **Éditeur d'archétype + save/load RON** : panneau egui à gauche, sliders
  bornés sur les gènes de l'archétype sélectionné, boutons Sauver/Charger. La
  distinction **archétype** (modèle édité) / **génome** (copie d'instance qui
  mute seule) est explicite.

### P2 — Interface complète — ✅ terminé

Voir, piloter, déboguer et rejouer une run déterministe — l'outillage du **groupe
témoin** :

- [x] **HUD courbes** (`src/hud.rs`) : population par espèce + dérive des gènes
  (normalisés dans leurs bornes), échantillonnés en temps simulé. Lecture seule.
- [x] **Contrôles de sim** (`src/controls.rs`) : pause, vitesse 0.5×–8×, pas-à-pas,
  reset — via `Time<Virtual>` (l'horloge fixe le suit) ; le reset reconstruit le
  monde depuis le `SimConfig`.
- [x] **Inspecteur d'agent** (`src/inspector.rs`) : clic → génotype, énergie,
  perception, action courante ; anneau de surlignage. Lecture seule.
- [x] **Runs & scénarios à chaud** (`src/runs.rs`) : sélecteur de `scenarios/*.ron`,
  recharge sans relancer le binaire, save/load d'un **snapshot de run** (état
  vivant sérialisé, cerveau compris — `src/snapshot.rs`).
- [x] **Arène en demi-espaces** (correctif) : plans infinis → les agents ne
  s'échappent plus (ni tunneling, ni naissance/dépose hors bord).

Suite (réorientée le 2026-06-15 — construire toute la stack *avant* l'intelligence
évoluée, pour tester avec un groupe témoin déterministe) : **P3** capture & vidéo
(re-render, pipe ffmpeg, run unique) → **P4** validation de l'abstraction (scénario
bataille générationnel, toujours déterministe) → **P5** intelligence évoluée,
dépriorisée (MLP, neuroévolution, parallélisme GA, NEAT). Voir
[`ROADMAP.md`](ROADMAP.md).

**Invariant cardinal :** aucune logique de simulation dans `Update`. L'agentivité
vit dans `FixedUpdate`, la physique Avian dans `FixedPostUpdate`. `Update` est
réservé au rendu / UI du binaire fenêtré.

## Architecture

```
src/
  lib.rs          SimPlugin : le cœur render-agnostic partagé.
  config.rs       SimConfig : le scénario (RON) + son chargement.
  components.rs    Corps de l'agent ; Vision (raycast) ; Species/Reserve ; Perception/Action = contrat du cerveau.
  brain.rs        Brain (enum, dispatch statique) ; WanderBrain déterministe.
  genotype.rs     Genotype héritable + mutation ; compilation génotype→phénotype (§2).
  snapshot.rs     Snapshot d'une run (état vivant sérialisable) : config + RNG + agents + nourriture.
  movement.rs     Systèmes percevoir / décider / agir (FixedUpdate, chaînés).
  interaction.rs  Primitive d'interaction unique (manger/attaquer) + table de relations.
  ecology.rs      Économie : métaboliser, mourir, se reproduire, réensemencer la nourriture.
  rng.rs          PRNG déterministe minimal (SplitMix64) + tirage gaussien.
  spawn.rs        Peuplement : arène + agents ; spawn_agent (compile un génotype).
  main.rs         Binaire fenêtré  → `teemlab`.
  editor.rs       UI egui (fenêtré seul) : palette d'archétypes + placement drag-and-drop.
  hud.rs          HUD egui (fenêtré seul) : courbes population + dérive des gènes (lecture seule).
  controls.rs     Contrôles egui (fenêtré seul) : pause / vitesse / pas-à-pas / reset (pilotage du temps).
  inspector.rs    Inspecteur egui (fenêtré seul) : clic → génotype / énergie / perception / action (lecture seule).
  runs.rs         Gestion egui (fenêtré seul) : sélecteur de scénario, recharge à chaud, save/load de run.
  bin/headless.rs Binaire headless → `headless`.
scenarios/
  default.ron     Scénario par défaut, tous champs documentés.
  crowded.ron     Variante (petite arène saturée) : override partiel.
  predation.ron   Deux espèces + une relation de prédation : démo de la primitive.
  selection.ron   Scénario nº1 : sélection naturelle (énergie, manger, mourir).
  evolution.ron   Boucle évolutive continue : reproduction + mutation des gènes.
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

cargo test                  # tests unitaires + intégration (confinement, snapshot)
```

Le build fenêtré ajoute, par-dessus la sim, l'outillage egui de P2 : bandeau de
contrôles (haut), éditeur d'archétypes + palette (gauche/droite), et fenêtres
flottantes HUD courbes / Inspecteur / Runs & scénarios. Tout cet outillage vit
hors `FixedUpdate` (rendu/UI) ; le headless, lui, n'embarque rien de tout ça.
