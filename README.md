# teemlab

Moteur de simulation évolutive. **Un seul moteur** interprète de la donnée ; chaque
simulation (sélection naturelle, bataille, …) est un *scénario*. Vue 2D top-down,
entités = ronds. Boucle unique : **percevoir → décider → agir**.

Conception et ordre d'implémentation : [`ROADMAP.md`](ROADMAP.md).

## État

**Réalisé (P0–P3).**

- **Fondations** : Bevy 0.18 + Avian 0.6, collisions, caméra 2D ; deux entrées (fenêtrée
  / headless) partageant le même schedule de sim à timestep fixe.
- **Boucle évolutive continue** : vision par raycast (avec coût métabolique), primitive
  d'interaction unique (prédation/combat), économie d'énergie (sélection naturelle),
  reproduction + mutation d'un génotype paramétrique. Scénario = donnée (RON, override
  partiel). `evolution.ron` : population stable, dérive des gènes observable.
- **Interface** (binaire fenêtré, egui) : HUD courbes, contrôles pause/vitesse/pas/reset,
  inspecteur d'agent, recharge de scénario à chaud, snapshot de run.
- **Capture vidéo** : rendu headless `record` → `ffmpeg` (re-render frais), menu
  d'enregistrement intégré.

**Planifié (P4–P5).** Sélection naturelle approfondie + intelligence évoluée en régime
continu (éditeur générique de caractéristiques avec flag *héritable*, `Brain::Hunter`
comme groupe témoin, scénarios co-évolutifs, MLP) ; puis la bataille (régime
générationnel) comme test final de l'abstraction, le long d'une *couture A/B* propre.

> **Invariant cardinal** : aucune logique de simulation dans `Update`. L'agentivité vit
> dans `FixedUpdate`, la physique Avian dans `FixedPostUpdate` ; `Update` est réservé au
> rendu / UI du binaire fenêtré.

## Architecture

```
src/
  lib.rs          SimPlugin : le cœur render-agnostic partagé.
  config.rs       SimConfig : le scénario (RON) + son chargement.
  components.rs   Corps de l'agent ; Vision (raycast) ; Species/Reserve ; Perception/Action = contrat du cerveau.
  brain.rs        Brain (enum, dispatch statique) ; WanderBrain déterministe.
  genotype.rs     Genotype héritable + mutation ; compilation génotype→phénotype (§2).
  snapshot.rs     Snapshot d'une run (état vivant sérialisable) : config + RNG + agents + nourriture.
  movement.rs     Systèmes percevoir / décider / agir (FixedUpdate, chaînés).
  interaction.rs  Primitive d'interaction unique (prédation/combat) + table de relations.
  ecology.rs      Économie : métaboliser, mourir, se reproduire, réensemencer la nourriture.
  rng.rs          PRNG déterministe minimal (SplitMix64) + tirage gaussien.
  spawn.rs        Peuplement : arène + agents ; spawn_agent (compile un génotype).
  main.rs         Binaire fenêtré  → `teemlab`.
  editor.rs       UI egui (fenêtré seul) : palette d'archétypes + placement drag-and-drop.
  hud.rs          HUD egui (fenêtré seul) : courbes population + dérive des gènes (lecture seule).
  controls.rs     Contrôles egui (fenêtré seul) : pause / vitesse / pas-à-pas / reset (pilotage du temps).
  inspector.rs    Inspecteur egui (fenêtré seul) : clic → génotype / énergie / perception / action (lecture seule).
  runs.rs         Gestion egui (fenêtré seul) : sélecteur de scénario, recharge à chaud, save/load de run.
  recorder.rs     Menu egui (fenêtré seul) : configure et lance le binaire `record` en sous-processus.
  visuals.rs      VisualsPlugin : rendu de la sim (mesh, arène, vision) partagé fenêtré ⇄ enregistreur.
  bin/headless.rs Binaire headless → `headless` (smoke test, sans rendu).
  bin/record.rs   Binaire d'enregistrement headless → `record` : rend sans fenêtre, pipe les frames sur ffmpeg.
scenarios/
  default.ron     Scénario par défaut, tous champs documentés.
  crowded.ron     Variante (petite arène saturée) : override partiel.
  predation.ron   Deux espèces + une relation de prédation : démo de la primitive.
  selection.ron   Scénario nº1 : sélection naturelle (énergie, manger, mourir).
  evolution.ron   Boucle évolutive continue : reproduction + mutation des gènes.
outputs/          Sorties des simulations (vidéos, images…) ; contenu ignoré par git.
```

## Développement

L'environnement (toolchain Rust + dépendances système de Bevy) est fourni par Nix :

```sh
nix develop            # ou : direnv allow  (puis automatique)

# Lancer le fenêtré — commande `play` du dev shell (voir l'encadré ci-dessous) :
play                                  # debug, scénario par défaut
play --release                        # release (teemlab ET record en release)
play --release scenarios/crowded.ron  # profil + scénario explicite

cargo run --bin headless                          # headless, scénario par défaut
cargo run --bin headless scenarios/default.ron    # scénario explicite (1ᵉʳ arg = RON)

# Enregistrer une run en vidéo (rendu headless → ffmpeg) ; sortie dans outputs/ :
cargo run --bin record -- scenarios/evolution.ron --out outputs/run.mp4 --fps 60 --seconds 10
#   options : --out F  --fps N  --seconds S  --width W  --height H

cargo test                            # tests unitaires + intégration (confinement, snapshot)
```

> **Lancer le fenêtré : la commande `play`** (fournie par le dev shell Nix —
> `flake.nix`, `writeShellScriptBin`, pas de script versionné). Le menu d'enregistrement
> lance `record` en sous-process, cherché *à côté* de l'exécutable courant. Or
> `cargo run --bin teemlab` ne compile QUE `teemlab` : sans un `record` buildé dans le
> même profil, l'enregistrement échoue (« No such file or directory »). `play` fait
> d'abord un `cargo build` (qui construit *tous* les binaires) dans le profil choisi, puis
> lance le fenêtré — `record` suit donc toujours `teemlab`, debug comme release.

Le build fenêtré ajoute, par-dessus la sim, l'outillage egui : bandeau de contrôles
(haut), éditeur d'archétypes + palette, et panneaux dockés HUD courbes / inspecteur /
runs & scénarios / enregistrement. Tout cet outillage vit hors `FixedUpdate` (rendu / UI) ;
le headless n'embarque rien de tout ça.
