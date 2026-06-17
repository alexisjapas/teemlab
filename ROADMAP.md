# Moteur de simulation évolutive — Synthèse de conception

> Document de référence. Vue 2D top-down, entités = ronds. **Un seul moteur** ; chaque
> simulation (sélection naturelle, bataille, …) est un *fichier de scénario*.

---

## 1. Principe directeur

Un seul **moteur** interprète de la donnée. La boucle est invariante — **percevoir → décider →
agir** — et ce qui varie d'un scénario à l'autre est la configuration, pas le code.

La modularité tient en **un axe à trois auteurs** (qui écrit le comportement et la structure ?) :

| Auteur | Moment | Décision via… | Corps via… |
|---|---|---|---|
| **Moteur** | compile-time | systèmes qui interprètent la donnée | composants et leurs effets |
| **Designer** | config-time | cerveau déterministe (règles) | valeurs de l'éditeur d'archétypes |
| **Évolution** | run-time | poids du réseau de neurones | gènes qui mutent |

L'axe s'applique deux fois : à la **décision** et au **corps**.

---

## 2. Contrats (invariants)

Les casser fait perdre la modularité.

- **Cerveau et corps = un contrat** : `floats normalisés en entrée → floats en sortie`. L'intérieur
  (réseau de neurones, arbre de décision, FSM) est interchangeable.
- **Stockage en `enum`, pas `Box<dyn>`** : dispatch statique, `serde` propre, `match` exhaustif
  vérifié à la compilation. Le crossover est intra-type (on ne croise pas un NN avec une FSM).
- **Le corps impose la forme des I/O du cerveau.** v1 : forme verrouillée par espèce ; les gènes
  font varier les *magnitudes* (portée de vision, vitesse), jamais le *nombre* de canaux. La
  topologie variable (NEAT) est repoussée.
- **Génotype ≠ phénotype** : on mute le génotype (description héritée), compilé en phénotype vivant
  (composants Avian + cerveau) au spawn. L'évolution ne touche jamais l'état physique courant.
- **Une caractéristique = (valeur, bornes, couplage de coût)** — plus, à l'édition, un facet
  **héritable ?** (participe-t-elle à l'hérédité, ou reste-t-elle fixée par l'archétype ?). Sans
  coût, tout converge vers le maximum et rien n'émerge ; le coût est défini par le scénario, pas
  par le moteur.

---

## 3. Primitive d'interaction unique

Manger et attaquer sont la même **interaction dirigée** : A réduit une ressource de B, à
portée/contact.

- **Prédation** : attaque qui *transfère* l'énergie à A.
- **Combat** : attaque qui *détruit* sans transfert.

Le moteur n'expose qu'**une primitive**. Le scénario en fixe la sémantique : la ressource (énergie /
PV), transfert ou non, et le filtre de cible (relation trophique prédateur→proie, ou camp
ennemi→ennemi). De même pour la perception : les requêtes spatiales sont de la machinerie moteur ;
le scénario choisit seulement *quels* canaux deviennent des entrées du cerveau.

---

## 4. Contrat de scénario et régimes évolutifs

Un scénario définit :

- **Spawn** : qui, où, combien, quels camps.
- **Vocabulaire** : actions et capteurs disponibles.
- **Table d'interactions** : qui agit sur qui, ressource visée, transfert ou non.
- **Couplages de coût** : ce que coûte chaque trait (vision → métabolisme, vitesse → énergie).
- **Conditions** : de mort, de fin.
- **Régime évolutif** : voir ci-dessous.

### Les régimes comme grille d'axes

Un régime n'est pas un atome mais un point dans une grille de deux axes largement indépendants :

- **Axe A — timing de reproduction** : *continu* (dans la sim, à la mort / à un seuil) ↔ *par lots*
  (à une frontière de génération, hors-sim).
- **Axe B — source de fitness** : *implicite / écologique* (émergente du monde) ↔ *explicite / par
  score* (calculée → sélection → reproduction).

| | Fitness implicite | Fitness explicite |
|---|---|---|
| **Repro continue** | **Sélection naturelle** | steady-state GA |
| **Repro par lots** | régime « saisonnier » | **Bataille** |

Les deux régimes canoniques occupent la diagonale ; les cases hors-diagonale sont des régimes
valides. Un continuum existe le long de l'axe A (*generation gap*). Les axes ne sont pas parfaitement
orthogonaux — la fitness implicite impose une sélection écologique —, ce qui fait des deux coins
diagonaux des configurations cohérentes ; l'axe A reste libre.

**Garde architecturale.** Ne pas réifier `enum Regime { Continuous, Generational }` : ce serait figer
le couplage dans le type (généralité ≠ modularité). Garder deux **coutures séparables** : « où vit la
reproduction » (système de sim en continu ↔ orchestrateur hors-sim en générationnel) et « d'où vient
la fitness » (émergente ↔ calculée). Critère de validité : un troisième régime doit être une
*recomposition* de ces pièces, jamais un cas spécial.

### Coexistence des types de cerveau

1. **Substitution** : échanger NN / déterministe par espèce (gratuit via le contrat).
2. **Cohabitation** : le déterministe sert de groupe témoin (un NN qui ne le bat pas n'a rien appris)
   et d'échafaudage (valider le pipeline avant que les NN existent).
3. **Hybridation** : réflexes en dur (fuir à PV critiques) court-circuitant la couche apprise
   (architecture de subsomption).

---

## 5. Stack technique

| Couche | Choix | Note |
|---|---|---|
| ECS / moteur | **Bevy 0.18** | adapté aux simulations lourdes |
| Physique | **Avian 0.6** | natif Bevy ; collisions **et** raycasting d'occlusion |
| HUD / courbes | **bevy_egui** | population, dérive des traits en temps réel |
| Sérialisation | **serde + RON** | archétypes lisibles ; binaire pour les snapshots |
| Cerveau | **fait maison** (MLP + mutation/crossover) | les libs ML visent le gros réseau GPU, l'inverse du besoin |
| Vidéo | **ffmpeg** | alimenté par re-render (§7) |

**Arbitrages :**

- **Performance > déterminisme strict** : parallélisme actif (intra- et inter-match), pas de
  `enhanced-determinism`.
- **Occlusion visuelle requise** : raycasting comme mécanisme de vision.
- **Timestep fixe** : pour la stabilité du solveur (un dt variable diverge), non pour le déterminisme.
- **Broad-phase Avian** comme structure de voisinage : pas de spatial hash maison.
- **RNG seedé** : pour rejouer une *configuration d'expérience* et comparer des paramètres, non pour
  la reproductibilité bit-à-bit (abandonnée avec le parallélisme).

---

## 6. Modèle d'exécution : headless ⇄ direct

Toute la logique de sim et la physique Avian vivent dans le schedule à timestep fixe (`FixedUpdate` /
`FixedPostUpdate`), identique avec ou sans fenêtre. Seuls changent le pilote de boucle et les plugins
de rendu.

- **Direct** : `DefaultPlugins` (winit pilote, rend, présente).
- **Headless** : `ScheduleRunnerPlugin`, sans fenêtre ni rendu.

> **Invariant : aucune logique de sim dans `Update`** (rendu, input, UI uniquement). Sinon le
> headless diverge du direct.

**Deux horloges** : la cadence de sim (timestep fixe, **64 Hz** par défaut) est constante et
indépendante de la cadence de rendu (`Update`, calée sur la vsync). Bevy exécute le schedule fixe 0,
1 ou plusieurs fois par frame pour rattraper le temps écoulé.

- **Débit headless** : piloter le schedule manuellement en boucle serrée (jusqu'à la condition de
  fin), pas via l'accumulateur temps-réel → nombre de ticks reproductible, vitesse maximale.
- **Pause / vitesse** : `Time<Virtual>::pause()` et `set_relative_speed(x)` (l'horloge fixe suit).
- **Spirale de la mort** : si un tick dépasse le temps réel, le rattrapage s'empile. `set_max_delta()`
  plafonne le rattrapage ; à régler quand le nombre d'entités croît.
- **Évolution générationnelle** : matchs headless parallélisés inter-matchs, un `World` isolé et une
  seed par match — c'est là que le débit croît.

---

## 7. Difficultés identifiées

- **Vidéo** : sans déterminisme, pas de rejeu par seed. Solution par défaut : re-render frais du
  meilleur génome (représentatif, pas le match historique exact). Alternative exacte : logger puis
  rejouer les trajectoires.
- **Vision par raycast** : goulot potentiel (N entités × M rayons × tick). Spatial queries Avian,
  rayons/portée plafonnés par espèce, vision traitée comme un coût pour borner la dérive.
- **Sélection naturelle** : le point de calibration central est l'**économie d'énergie**. Mal
  calibrée → effondrement ou explosion ; cycles de Lotka-Volterra (proie-prédateur) à stabiliser.
- **Bataille** : le comportement émergent reflète la **fonction de fitness** (récompenser les kills →
  kamikazes ; la survie → évitement). Co-évolution des camps → instabilité (Reine Rouge).

---

## 8. Ordre d'implémentation

Principe : bâtir la fondation découplée d'abord, valider chaque tranche avec des agents déterministes
(échafaudage), réaliser un scénario de bout en bout avant de généraliser. Le second scénario d'un
type donné sert de test : si l'abstraction tient, il est presque entièrement de la configuration.

Trois principes de méthode :

- **Généralité ≠ modularité** : un mécanisme général peut être profondément couplé ; la modularité ne
  se falsifie que contre la **pluralité** (≥ 2 instances par axe).
- **Éditeur piloté par les scénarios** : chaque brique naît d'un besoin réel et prouvée modulaire ;
  « éditeur complet » est un résultat, pas un préalable.
- **Stub le comportement, jamais le schéma** : une coquille de comportement (cerveau no-op) est un
  échafaudage légitime ; une coquille de contrat de données fige la mauvaise forme — la forme du
  schéma *est* l'abstraction.

Objectif : une **plateforme d'expériences** mesurant ce qu'un cerveau appris apporte face à un groupe
témoin déterministe. La sélection naturelle (régime continu) est approfondie en premier ; elle porte
déjà prédation, compétition et co-évolution (cf. Avida, Tierra, Polyworld). Le régime générationnel
(bataille) est différé comme test final de l'axe A.

### P0 — Fondations (réalisé)

1. Bevy + Avian, ronds rigides, collisions, caméra 2D ; sim dans `FixedUpdate` / `FixedPostUpdate`.
2. Boucle percevoir→décider→agir avec un cerveau déterministe trivial (errance).
3. Deux entrées partageant le même schedule : direct (`DefaultPlugins`) et headless
   (`ScheduleRunnerPlugin`, comptage de ticks fixes jusqu'à la condition de fin).

### P1 — Moteur jouable : boucle évolutive continue (réalisé)

4. Placement : drag-and-drop manuel + spawn aléatoire en nombre (éditeur fenêtré).
5. Éditeur d'archétype + save/load RON ; distinction archétype (config) / génome (instance).
6. Vision par raycast avec occlusion (spatial queries Avian) ; coût métabolique couplé portée × rayons.
7. Primitive d'interaction unique (prédation/combat) + table de relations par espèce.
8. Scénario nº1 — sélection naturelle : métabolisme, alimentation, mort à zéro, réensemencement.
9. Reproduction + mutation d'un génotype paramétrique → boucle évolutive continue ; repousse à débit
   fini → capacité de charge (`scenarios/evolution.ron` : population stable, dérive des gènes).

### P2 — Interface (réalisé)

Outillage d'observation et de pilotage, entièrement dans le binaire fenêtré (`Update` / egui).

10. HUD / courbes : population par espèce, dérive des traits normalisés (lecture seule).
11. Contrôles : pause, vitesse 0.5×–8×, pas-à-pas, reset (pilotage de `Time<Virtual>` ; le reset
    reconstruit le monde depuis `SimConfig`).
12. Inspecteur d'agent : génotype, réserve, perception, action courante (lecture seule).
13. Runs/scénarios à chaud : sélecteur RON, recharge sans relancer le binaire, snapshot de run sérialisé.

### P3 — Capture vidéo (réalisé)

14. Rendu headless → `ffmpeg` (pipe direct des frames, sans PNG intermédiaire ; re-render frais).
    Menu d'enregistrement intégré au build fenêtré (lance `record` en sous-process). Rendu de la sim
    factorisé (`VisualsPlugin`) partagé fenêtré ⇄ enregistreur.

### P4 — Sélection naturelle approfondie + intelligence évoluée (régime continu)

L'évolution d'intelligence est la frontière de l'abstraction *dans* la sélection naturelle. L'éditeur
grandit ici, piloté par ces scénarios.

15. Éditeur générique de caractéristiques : (valeur, bornes) + toggle « héritable ? » par trait, et
    sélecteur de cerveau (chaque variant de `Brain` exposant ses propres paramètres éditables).
    Testable immédiatement contre la pluralité de traits existante.
16. `Brain::Hunter` déterministe : réflexe utilisant la perception (orientation vers la cible perçue
    la plus proche, attaque au contact). Groupe témoin compétent ; rend le chemin
    percevoir→décider→agir porteur et le sélecteur de cerveau falsifiable (2ᵉ variant de `Brain`).
17. Pluralité de scénarios de sélection naturelle (dont un proie-prédateur co-évolutif) + calibration
    de l'économie (cycles de Lotka-Volterra). Critère de falsification explicite par scénario : bande
    de population, dérive attendue, et « scénario ajouté en donnée + un driver, zéro édition de
    `movement` / `interaction` / `ecology` ».
18. MLP fait maison + neuroévolution, en substitution par espèce, dans le régime continu. Mutation
    gaussienne sur les poids ; crossover paramétrique (gènes) trivial. Le déterministe reste le
    groupe témoin.

### P5 — Bataille (différée) + passage à l'échelle

Le régime générationnel teste l'axe A : il doit entrer comme recomposition le long de la couture A/B
(§4), sans toucher de système cœur.

19. Scénario bataille — régime générationnel : boucle run → score → breed → run (orchestrateur
    hors-sim), fitness explicite via un menu de primitives moteur, condition terminale, camps
    (= espèces + relation `transfer: false`).
20. Headless parallélisé inter-matchs : `World` isolés, batch multi-cœurs.
21. Hybridation réflexes/appris (subsomption) ; topologie variable / NEAT, si une morphologie à
    nombre de capteurs variable se confirme nécessaire.

---

## 9. Points techniques ouverts

- **Couture de régime A/B** (§4) : en continu, la reproduction est un système de sim
  (`ecology::reproduce`, `FixedUpdate`) à fitness implicite ; le générationnel ajoute un orchestrateur
  hors-sim sans que le système continu en dépende. Pas d'`enum Regime` fermé.
- **Stepping manuel headless** : `app.update()` en boucle serrée exige `app.finish()` puis
  `app.cleanup()` au préalable (Avian insère des ressources dans `Plugin::finish()`). Éprouvé dans
  `tests/containment.rs` et `tests/snapshot.rs`.
- **MLP** (item 18) : nouveau variant de `Brain` (enum déjà `serde`) sur le contrat
  `Perception → Action`, déjà matérialisé par les systèmes perceive/decide/act. Arrive en régime
  continu, en substitution.
- **Crossover** : paramétrique (gènes) trivial et sûr ; sur poids de NN, problème de permutation
  (conventions concurrentes) → repoussé avec NEAT, neuroévolution mutation-seule d'abord.
- **Capture multi-runs et re-render du meilleur génome** : pertinents une fois la sélection
  générationnelle et le batch inter-matchs en place (P5).
- **Repli des valeurs fondatrices** (item 15) : `SimConfig` porte aujourd'hui les valeurs
  d'archétype en champs épars (`max_speed`, `agility`, …) qui doublent ceux du `Genotype`. Les
  replier en un seul `founder: Genotype` supprimerait les accesseurs `base`/`set_base` et cette
  duplication, mais casse le RON de tous les scénarios (champs de premier niveau → imbriqués sous
  `founder`). Différé hors de l'amorce item 15 ; à faire avec une migration des `.ron` versionnés.
