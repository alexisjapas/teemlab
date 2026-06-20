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

**P4 — sélection naturelle approfondie + intelligence évoluée (réalisé).** Régime continu.

- **Gènes génériques** : table `TRAITS` (valeur, bornes, facet *mutable ?* **par espèce**),
  exposée sans code dédié par l'éditeur / le HUD / l'inspecteur. Reproduction, métabolisme,
  locomotion, **précision visuelle** (`vision_rays`) et **photosynthèse / dissémination**
  (flore) sont des gènes ; généalogie (génération, âge) à l'inspecteur.
- **Cerveaux** (`Brain`, enum à dispatch statique), **par espèce** et **hérités** à la
  reproduction : `Wander` (témoin naïf), `Hunter` (témoin compétent — foncer vers la cible
  perçue **et fuir les menaces** : canaux *cible* / *menace* de la perception), `Sessile`
  (flore), **`Mlp`** (perceptron fait maison **appris par neuroévolution**, lisant les mêmes
  canaux *vision/cible/menace* — il peut donc *apprendre* à fuir —, avec graphe d'activations à
  l'inspecteur). Sélecteur de cerveau dans l'éditeur.
- **Scénarios pilotes**, tous robustes sur multi-graines via leurs drivers :
  `proie_predateur` (chaîne trophique à 3 niveaux, effectif par espèce, proies qui fuient),
  `cohabitation` & `cerveau_mlp` (témoin vs appris → exclusion compétitive).

**« Tout est entité » (réalisé).** L'espèce (`Archetype`) est la donnée **centrale** du
scénario : corps + cerveau + gènes + effectif, et son index est son identité. Éditeur
complet — créer / dupliquer / réordonner / supprimer, **bibliothèque d'espèces**
réutilisables (`species/*.ron`, import par copie + resynchronisation), et tous les
paramètres de monde dans l'UI (dont `tick_hz` et les bornes de gènes). **Flore évolutive** :
une plante sessile vit de photosynthèse, se sème localement et s'auto-limite par compétition
intraspécifique — la primitive d'interaction réutilisée, sans mécanisme nouveau.

**Reste.** Dissoudre le type spécial `Food` (devenu le cas dégénéré d'une flore) ; **P5 —
bataille** (régime générationnel, test final de l'abstraction le long d'une *couture A/B*
propre).

> **Invariant cardinal** : aucune logique de simulation dans `Update`. L'agentivité vit
> dans `FixedUpdate`, la physique Avian dans `FixedPostUpdate` ; `Update` est réservé au
> rendu / UI du binaire fenêtré.

## Architecture

```
src/
  lib.rs          SimPlugin : le cœur render-agnostic partagé.
  config.rs       SimConfig : le scénario (RON) + chargement ; Archetype (espèce de 1er ordre : corps + cerveau + gènes), import/export d'espèces ; table de relations ; bornes de gènes.
  components.rs   Corps de l'agent ; Vision (raycast) ; Species/Reserve ; Perception (canaux vision/cible/menace) / Action = contrat du cerveau ; généalogie (Generation/Age).
  brain.rs        Brain (enum, dispatch statique) : Wander (errance) · Hunter (chasse + fuite) · Sessile (flore) · Mlp (appris, neuroévolution) ; BrainKind = choix de scénario.
  genotype.rs     Genotype héritable (table TRAITS générique) + mutation ; compilation génotype→phénotype (§2).
  snapshot.rs     Snapshot d'une run (état vivant sérialisable) : config + RNG + agents + nourriture.
  movement.rs     Systèmes percevoir / décider / agir (FixedUpdate, chaînés).
  interaction.rs  Primitive d'interaction unique (prédation / combat / compétition) + table de relations.
  ecology.rs      Économie : métaboliser (dépenses + photosynthèse), mourir, vieillir, se reproduire (semis local), réensemencer la nourriture.
  rng.rs          PRNG déterministe minimal (SplitMix64) + tirage gaussien.
  spawn.rs        Peuplement : arène + agents ; spawn_agent (compile un génotype en phénotype vivant).
  main.rs         Binaire fenêtré  → `teemlab`.
  editor.rs       UI egui (fenêtré seul) : palette (créer / dupliquer / réordonner / supprimer, placement glisser-déposer, Suppr retire), bibliothèque d'espèces (species/*.ron), éditeur du Monde (arène, tick_hz, bornes de gènes, relations).
  hud.rs          HUD egui (fenêtré seul) : courbes population + dérive des gènes (lecture seule).
  controls.rs     Contrôles egui (fenêtré seul) : pause / vitesse / pas-à-pas / reset (pilotage du temps, ré-applique tick_hz).
  inspector.rs    Inspecteur egui (fenêtré seul) : clic → génotype / énergie / perception / action / graphe MLP / généalogie (lecture seule).
  runs.rs         Gestion egui (fenêtré seul) : sélecteur de scénario, recharge à chaud, save/load de run.
  recorder.rs     Menu egui (fenêtré seul) : configure et lance le binaire `record` en sous-processus.
  visuals.rs      VisualsPlugin : rendu de la sim (mesh, arène, vision) partagé fenêtré ⇄ enregistreur.
  bin/headless.rs Binaire headless → `headless` (smoke test, sans rendu).
  bin/record.rs   Binaire d'enregistrement headless → `record` : rend sans fenêtre, pipe les frames sur ffmpeg.
scenarios/
  default.ron     Scénario par défaut, tous champs documentés.
  empty.ron       Arène vide : la toile de l'éditeur (repli sans-argument du fenêtré).
  evolution.ron   Boucle évolutive continue : reproduction + mutation des gènes (cerveaux d'errance).
  chasse.ron      Cerveaux Hunter sur une nourriture : le groupe témoin compétent (item 16).
  cohabitation.ron     Témoin compétent (Hunter) vs naïf (errance), même corps : exclusion compétitive (item 18a).
  cerveau_mlp.ron      Cerveau APPRIS (MLP) vs errance : domine en partant de poids aléatoires (item 18b).
  proie_predateur.ron  Chaîne trophique à 3 niveaux (plantes → proies → prédateurs) : pyramide
                  par effectifs, cerveaux Hunter, proies qui fuient (items 17, 18e).
  flore.ron       Flore sessile auto-limitée : photosynthèse + semis local + compétition (item 5, Phase 3a).
species/
  chasseur.ron    Espèce réutilisable (bibliothèque) : un chasseur générique, importable dans un scénario.
outputs/          Sorties des simulations (vidéos, images…) ; contenu ignoré par git.
```

## Développement

L'environnement (toolchain Rust + dépendances système de Bevy) est fourni par Nix :

```sh
nix develop            # ou : direnv allow  (puis automatique)

# Lancer le fenêtré — commande `play` du dev shell (voir l'encadré ci-dessous) :
play                                  # debug, arène vide (toile de l'éditeur)
play scenarios/evolution.ron          # debug, scénario explicite
play --release                        # release (teemlab ET record en release)
play --release scenarios/flore.ron    # profil + scénario explicite

cargo run --bin headless                          # headless, scénario par défaut
cargo run --bin headless scenarios/default.ron    # scénario explicite (1ᵉʳ arg = RON)

# Enregistrer une run en vidéo (rendu headless → ffmpeg) ; sortie dans outputs/ :
cargo run --bin record -- scenarios/evolution.ron --out outputs/run.mp4
#   options : --out F  --fps N  --seconds S  --width W  --height H
#   (défauts : 30 fps, 61 s, 1080×1080 — l'arène est carrée)

cargo test                            # tests unitaires + drivers multi-graines + snapshot/confinement
cargo fmt                             # formatage — rustfmt par défaut fait foi
cargo clippy --all-targets            # lint — l'arbre est tenu à zéro warning
```

> **Convention de format.** On suit le **formatter de cargo** (`cargo fmt`,
> rustfmt par défaut) : pas de `rustfmt.toml`, c'est l'outil qui tranche. Tout
> commit doit laisser `cargo fmt --check` propre (et `cargo clippy --all-targets`
> sans warning). On formate donc *avant* de committer plutôt que d'aligner à la
> main — la mise en page n'est pas un terrain de revue.

> **Lancer le fenêtré : la commande `play`** (fournie par le dev shell Nix —
> `flake.nix`, `writeShellScriptBin`, pas de script versionné). Le menu d'enregistrement
> lance `record` en sous-process, cherché *à côté* de l'exécutable courant. Or
> `cargo run --bin teemlab` ne compile QUE `teemlab` : sans un `record` buildé dans le
> même profil, l'enregistrement échoue (« No such file or directory »). `play` fait
> d'abord un `cargo build` (qui construit *tous* les binaires) dans le profil choisi, puis
> lance le fenêtré — `record` suit donc toujours `teemlab`, debug comme release.

Le build fenêtré ajoute, par-dessus la sim, l'outillage egui : bandeau de contrôles
(haut), éditeur d'archétypes + palette (glisser-déposer pour poser, **Suppr** pour
retirer l'entité sous le curseur), éditeur du **Monde** (arène, nourriture, table de
relations), et fenêtres flottantes HUD courbes / inspecteur / runs & scénarios /
enregistrement. Tout cet outillage vit hors `FixedUpdate` (rendu / UI) ; le headless
n'embarque rien de tout ça.
