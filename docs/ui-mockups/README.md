# UI design comp (Claude Design)

High-fidelity visual mockups of the five-screen redesign
(**Observe · Library · Studio · Lab · Analyze**). Open `teemlab.dc.html` in a
browser to view; the nav rail switches screens.

**This is a visual reference, not implementable assets.** The app is Rust + egui;
these HTML / React / CSS comps fix the *look and structure* only. Behaviour, live
data binding, and the simulation model they depict are built per
[`../ui-redesign.md`](../ui-redesign.md) (the screen/needs spec) and
[`../emergent-trophics.md`](../emergent-trophics.md) (the simulation redesign).

Files:

- `teemlab.dc.html` — the comp (all five screens + a theme switcher).
- `support.js` — the design-comp runtime (generated; renders the `.dc.html`).
- `assets/` — referenced images.

Known reconciliations the comp has not yet absorbed (see `../ui-redesign.md`
"Visual reference"): the per-archetype gene panel still shows the old free cost
genes that the allometric law replaces with derived read-only costs; the theme
switcher is exploration (ship one theme at MVP).
