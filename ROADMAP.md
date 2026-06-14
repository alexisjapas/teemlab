# Moteur de simulation évolutive — Synthèse de conception

> Document de référence. Vue 2D top-down, entités = ronds. **Un seul moteur**, dont chaque
> simulation (sélection naturelle, bataille, …) n'est qu'un *fichier de scénario*.

> **Statut au 2026-06-14.** P0 (fondations) terminé. P1 **quasi complet** : la première
> **boucle évolutive continue** tourne de bout en bout — vision par raycast (item 6),
> primitive d'interaction unique (item 7), économie d'énergie / sélection naturelle (item 8),
> reproduction + mutation d'un génotype paramétrique (item 9). Vérifié sur `evolution.ron` :
> population stable et dérive des gènes observable. Ne restent du P1 que des conforts d'UI hors
> du chemin critique : le **placement drag-and-drop** (item 4) et l'**éditeur d'archétype**
> (item 5). La numérotation de phase de ce document fait foi.

---

## 1. Vision et principe directeur

On ne construit pas plusieurs simulateurs, on construit **un moteur** qui *interprète de la
donnée*. La boucle est toujours la même — **percevoir → décider → agir** — et ce qui change
d'un scénario à l'autre n'est pas le code mais la configuration.

La modularité se résume à **un axe à trois auteurs** : *qui écrit le comportement et la
structure ?*

| Auteur | Moment | Écrit la décision via… | Écrit le corps via… |
|---|---|---|---|
| **Moteur** | compile-time | les systèmes qui interprètent la donnée | les composants et leurs effets |
| **Designer** | config-time | cerveau déterministe (règles) | valeurs de l'éditeur d'archétypes |
| **Évolution** | run-time | poids du réseau de neurones | gènes qui mutent |

Le même axe s'applique deux fois : une fois à la **décision**, une fois au **corps**. Tout le
reste du design découle de là.

---

## 2. Les contrats à protéger

Ce sont les invariants. Les casser, c'est perdre la modularité.

- **Cerveau et corps = un seul contrat** : `floats normalisés en entrée → floats en sortie`.
  L'intérieur (réseau de neurones, arbre de décision, FSM) est interchangeable.
- **Stockage en `enum`, pas en `Box<dyn>`** : dispatch statique, `serde` propre, et le
  compilateur liste les `match` à compléter quand on ajoute un module. Le crossover est
  intrinsèquement de même type (on ne croise pas un NN avec une FSM), ce qui casse de toute
  façon la compatibilité `dyn`.
- **C'est le corps qui impose la forme des I/O du cerveau.** v1 : forme **verrouillée par
  espèce**, les gènes font varier les *magnitudes* (portée de vision, vitesse…), jamais le
  *nombre* de canaux. La topologie variable (NEAT) est le mode hard, repoussé.
- **Génotype ≠ phénotype** : on mute le **génotype** (la description héritée), puis on le
  *compile* vers le phénotype vivant (composants Avian + cerveau) au spawn. L'évolution ne
  touche jamais l'état physique en cours.
- **Une caractéristique n'est pas un nombre** mais un triplet **(valeur, bornes, couplage de
  coût)**. Sans coût, tout converge vers « tout au max » et rien n'émerge. Le coût est défini
  par le *scénario*, pas par le moteur.

---

## 3. L'insight unificateur : *manger et attaquer sont le même verbe*

Les deux sont une **interaction dirigée** où A réduit une ressource de B, à portée/contact.

- **Prédation** = attaque qui *transfère* l'énergie au prédateur.
- **Combat** = attaque qui *détruit* sans transfert.

Le moteur n'a donc qu'**une primitive d'interaction**. Le scénario en configure la sémantique :
quelle ressource (énergie / PV), transfert ou non, et quel filtre de cible (relation trophique
prédateur→proie, ou filtre de camp ennemi→ennemi). Idem pour la perception : les requêtes
spatiales sont de la machinerie *moteur* partagée ; le scénario choisit seulement *quels*
canaux deviennent des entrées du cerveau.

---

## 4. Le contrat de Scénario

Un scénario est un fichier qui définit :

- **Spawn** : qui, où, combien, quels camps.
- **Vocabulaire** : actions et capteurs disponibles.
- **Table d'interactions** : qui peut agir sur qui, ressource visée, transfert ou non.
- **Couplages de coût** : ce que chaque trait coûte (vision → métabolisme, vitesse → énergie…).
- **Condition de mort** et **condition de fin**.
- **Régime évolutif** : continu-implicite **ou** générationnel-explicite (+ fonction de fitness
  si explicite).

### Les deux régimes évolutifs

| | Sélection naturelle | Bataille |
|---|---|---|
| Fitness | implicite, endogène (« tu t'es reproduit ») | explicite, que tu définis |
| Reproduction | dans la sim, à tout moment | entre deux runs |
| Évolution | continue, steady-state | générationnelle (run → score → breed → run) |
| Fin | aucune / extinction | état terminal (un camp debout) |

C'est la vraie exigence imposée à l'architecture : accueillir *ces deux boucles évolutives*
sous une interface commune.

### Coexistence des deux types de cerveau

1. **Substitution** — échanger NN/déterministe par espèce (gratuit via le contrat).
2. **Cohabitation** — le déterministe sert de **groupe témoin** (si le NN ne le bat pas, il n'a
   rien appris) et d'**échafaudage** (valider tout le pipeline avant que les NN existent).
3. **Hybridation** — réflexes en dur (fuir à PV critiques) court-circuitant la couche apprise
   (architecture de subsomption).

---

## 5. Stack technique

| Couche | Choix | Note |
|---|---|---|
| ECS / moteur | **Bevy 0.18** | stable actuel ; adapté aux sims lourdes |
| Physique | **Avian 0.6** | natif Bevy ; porte collisions **et** raycasting d'occlusion |
| HUD / courbes | **bevy_egui** | population, dérive des traits en temps réel |
| Sérialisation | **serde + RON** (archétypes), binaire (snapshots) | RON lisible/commentable |
| Cerveau | **fait maison** (MLP + mutation/crossover) | libs ML = gros réseau GPU, l'inverse du besoin ; crates NEAT à l'abandon |
| Vidéo | **ffmpeg** | alimenté par re-render (voir §7) |

### Décisions d'arbitrage tranchées

- **La performance prime sur le déterminisme strict.** → parallélisme actif (intra- et
  inter-match), pas de `enhanced-determinism`.
- **L'occlusion visuelle est requise.** → raycasting confirmé comme mécanisme de vision.

### Configuration Avian (orientée performance)

- **Timestep fixe conservé** — non pour le déterminisme mais pour la *stabilité du solveur*
  (un dt variable le fait diverger).
- **`enhanced-determinism` abandonné**, **parallélisme laissé actif**.
- **Spatial hash maison supprimé** : la broad-phase d'Avian *est* la structure de voisinage.
- **RNG seedé** conservé — non pour la reproductibilité bit-à-bit (abandonnée) mais pour
  rejouer une *config d'expérience* et comparer des changements de paramètres.

---

## 6. Exécution : headless ⇄ direct, et gestion des FPS

Toute la logique de sim + la physique Avian vivent dans le **schedule à timestep fixe**
(`FixedUpdate` / `FixedPostUpdate`), identique avec ou sans fenêtre. Seuls changent le *pilote
de boucle* et les *plugins de rendu*.

- **Direct** : `DefaultPlugins` (winit pilote, rend, présente).
- **Headless** : retirer fenêtre/rendu, utiliser `ScheduleRunnerPlugin`.

> **Règle absolue, jamais enfreinte : aucune logique de sim dans `Update`.** `Update` =
> rendu, input, UI uniquement. Sinon le headless diverge du direct.

### Deux horloges distinctes

- **Cadence de sim** = timestep fixe (**64 Hz** par défaut, ajustable). Constante, indépendante
  du rendu.
- **Cadence de rendu** = schedule `Update`, une fois par frame, calée sur la vsync.

À chaque frame, Bevy exécute le schedule fixe **0, 1 ou plusieurs fois** pour rattraper le
temps écoulé. La physique reste cohérente quelle que soit la fluidité de l'affichage.

### Débit headless

Ne pas s'appuyer sur l'accumulateur temps-réel : **piloter le schedule de sim manuellement**
dans une boucle serrée (jusqu'à la condition de fin du match). Nombre de ticks reproductible,
vitesse maximale.

### Bonus gratuits (l'horloge fixe suit `Time<Virtual>`)

- **Pause** : `Time<Virtual>::pause()` — sim figée, rendu actif.
- **Accéléré / ralenti** : `set_relative_speed(x)` pour observer l'évolution.

### Le piège : la spirale de la mort

Si un tick devient plus lent que le temps réel, Bevy empile les ticks de rattrapage → blocage.
**`set_max_delta()` est la soupape** : il plafonne le rattrapage, la sim ralentit proprement.
À régler dès qu'on pousse le nombre d'entités.

### Modèle d'exécution résumé

- **Interactif** : un match, parallélisme intra-match, rendu live, pause/vitesse offerts.
- **Évolution générationnelle** : matchs headless parallélisés **inter-matchs** sur tous les
  cœurs, chaque match son `World` isolé et sa seed. C'est là que le débit explose.

---

## 7. Les coutures honnêtes (les points qui coûteront du temps)

- **Vidéo** : sans déterminisme, **pas de rejeu gratuit par seed**. Solution par défaut :
  **re-render frais du meilleur génome** (relancer une run du gagnant avec rendu — représentatif,
  pas le match historique exact). Alternative exacte : logger les trajectoires puis les rejouer.
- **Vision par raycast** : futur goulot (N entités × M rayons × tick). Utiliser les *spatial
  queries* d'Avian, plafonner rayons/portée par espèce, et **traiter la vision comme un coût**
  (plus de portée = plus de métabolisme) pour borner la dérive.
- **Sélection naturelle** : tout est dans l'**économie d'énergie**. Mal calibrée → effondrement
  ou explosion. Cycles de Lotka-Volterra (proie-prédateur) à apprivoiser. *Du réglage, pas de
  l'algo.*
- **Bataille** : tout est dans la **fonction de fitness** — *tu obtiens ce que tu récompenses*.
  Kills → kamikazes ; survie → planqués. Co-évolution des deux camps → instabilité (Reine Rouge).

---

## 8. Priorisation d'implémentation

Principe de l'ordre : **bâtir la fondation découplée d'abord**, valider chaque tranche avec des
agents **déterministes** (l'échafaudage), faire **un scénario de bout en bout** avant de
généraliser, et n'ajouter le headless/parallèle qu'une fois une run complète fonctionnelle. Le
**second scénario sert de test** : si l'abstraction tient, il n'est presque que de la config.

Légende : `[x]` fait · `[~]` partiel · `[ ]` à faire.

### P0 — Fondations (rien n'a de sens sans ça) — ✅ terminé

- [x] **1. Bevy + Avian, ronds rigides, collisions, caméra 2D top-down.** Découplage validé :
  sim dans `FixedUpdate`/`FixedPostUpdate`, rien dans `Update` sauf rendu.
- [x] **2. Boucle percevoir→décider→agir avec un cerveau déterministe trivial** (errance +
  steering par braquage de vitesse). L'échafaudage qui dérisque tout le reste.
- [x] **3. Skeleton des deux `main()`** : direct (`DefaultPlugins`) vs headless
  (`ScheduleRunnerPlugin` + comptage de ticks fixes jusqu'à la condition de fin), partageant le
  *même* schedule de sim. *(Reste à muscler : stepping manuel `app.update()` en boucle serrée
  pour l'évolution générationnelle — voir item 13.)*

### P1 — Le moteur jouable (une boucle évolutive complète)

- [~] **4. Placement** : drag-and-drop manuel + spawn aléatoire en nombre.
  *(Fait : spawn aléatoire en nombre — `agent_count`, positions seedées. À faire :
  drag-and-drop manuel.)*
- [~] **5. Éditeur d'archétype + save/load RON.** Distinguer archétype (config éditable) et
  génome (valeurs d'instance).
  *(Fait : plomberie save/load RON — `serde` + `ron`, `SimConfig` chargé depuis un `.ron`,
  override partiel, scénario partagé par les deux binaires. À faire : l'éditeur d'archétype et
  la distinction explicite archétype/génome.)*
- [~] **6. Vision par raycast avec occlusion** (spatial queries Avian), avec coût métabolique.
  *(Fait : éventail de rayons par agent via `SpatialQuery`, occlusion intrinsèque
  — chaque rayon ne garde que le hit le plus proche, donc un mur masque ce qui
  est derrière —, proximité normalisée écrite dans `Perception`, rendu des rayons
  en build fenêtré pour vérifier l'occlusion à l'œil. Le **coût métabolique** est
  quantifié — `Vision::metabolic_cost()`, couplé portée × rayons — mais pas encore
  prélevé : son consommateur est l'économie d'énergie de l'item 8. Forme du
  capteur verrouillée par espèce v1 ; les magnitudes viendront des gènes.)*
- [~] **7. Primitive d'interaction unique** (manger/attaquer) + table de relations.
  *(Fait : un seul système `interact` — un acteur réduit la `Reserve` d'une cible
  à portée (broad-phase Avian), `transfer: true` = prédation (gain pour l'acteur),
  `false` = combat. `Species` filtre les cibles ; la table `relations` (RON) dit
  qui agit sur qui. Substrat posé : `Reserve` générique (énergie *ou* PV, au choix
  du scénario), spawn multi-espèces, `scenarios/predation.ron`. À faire avec
  l'item 8 : mort à zéro, régénération/métabolisme et la distinction explicite
  énergie/PV — donc l'économie calibrée, pas juste la mécanique.)*
- [~] **8. Scénario nº1 — sélection naturelle**, agents déterministes : énergie, manger, mourir.
  Calibrer l'économie d'énergie ici (le vrai travail).
  *(Fait : économie complète — métabolisme (`ecology::metabolize` : base +
  locomotion + **coût de vision** de l'item 6, qui trouve enfin son consommateur),
  nourriture comme réserve mangée via l'unique primitive d'interaction (item 7),
  mort à zéro (`reap`), réensemencement (`replenish_food`). `scenarios/selection.ron`
  calibré : ~36/40 survivants à 100 s, énergie ~87 — l'économie soutient les
  butineurs, les malchanceux meurent de faim. Reste ouvert : la **persistance
  vraie** (la population ne fait que décliner sans reproduction) et donc le réglage
  fin des cycles Lotka-Volterra arrivent avec la boucle de l'item 9 ; le calibrage
  ici établit une économie viable, pas encore un équilibre auto-entretenu.)*
- [x] **9. Reproduction + mutation** (génotype paramétrique d'abord). On a alors une *boucle
  évolutive continue* complète.
  *(Fait : `Genotype` héritable (gènes = magnitudes : vitesse, agilité, portée et
  champ de vision) ; §2 respecté — on mute le génotype, **compilé** en phénotype
  au spawn (`spawn_agent` partagé par peuplement et reproduction). `ecology::reproduce`
  (régime continu-implicite) : à seuil d'énergie, un parent paie `offspring_energy`
  pour engendrer un enfant muté (gaussienne bornée). Coût du gène de vitesse
  corrigé (couplé à la vitesse absolue). Repousse de nourriture à débit fini →
  **capacité de charge** (sinon la population explose). `scenarios/evolution.ron`,
  vérifié : population stable ~90, et dérive nette des gènes sur 100 s — la
  **portée de vision baisse** (pur coût tant que l'errance l'ignore : le cadre
  minimise un trait coûteux et inutile), la **vitesse monte** (meilleur butinage).
  C'est la première boucle évolutive continue complète.)*

### P2 — Intelligence évoluée et passage à l'échelle

- [ ] **10. MLP fait maison** branché sur le contrat I/O, en remplacement par espèce
  (substitution).
- [ ] **11. Neuroévolution** (mutation gaussienne + crossover sur les poids). Le déterministe
  reste le *groupe témoin*.
- [ ] **12. HUD / courbes egui** : population par espèce, dérive des traits moyens — *voir*
  l'évolution.
- [ ] **13. Headless parallélisé inter-matchs** + génération vidéo par re-render du meilleur
  génome.

### P3 — Validation de l'abstraction et raffinements

- [ ] **14. Scénario nº2 — bataille** : régime générationnel, fitness explicite. *Test ultime* :
  si ça tient presque en config seule, l'architecture est saine.
- [ ] **15. Hybridation** réflexes/appris (subsomption) ; co-évolution des camps (en
  connaissance de l'instabilité Reine Rouge).
- [ ] **16. Topologie variable / NEAT** (mode hard) — seulement si le besoin d'une morphologie à
  nombre de capteurs variable se confirme.

---

## 9. Fils techniques ouverts (pour la suite)

- Modèle d'**isolation des `World`** pour paralléliser les matchs du GA.
- Design du **`sense()` / `actuate()`** branchant cerveau et corps sur les raycasts et forces
  d'Avian.
- Squelette concret des deux `main()` + branchement du stepping manuel sur la condition de fin.
- Encodage du **génotype** (poids, plage de mutation) et stratégie de **crossover**.
