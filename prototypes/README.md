# Prototypes — launcher redesign mockups

Static, clickable mockups of possible launcher redesigns. **Not part of the app.**

- Lives outside `src/` on purpose — Tauri's `frontendDist` is `../src`, so nothing
  here is ever bundled into a release. The app itself is untouched.
- Each file links the real `../src/style.css`, so tokens, themes and components
  look exactly native; the only new CSS is the shell being tested.
- All data is fake. No Tauri, no `w2gp.js` bridge, no backend calls.

Open by double-clicking the file (any browser), or in the OpenCode review pane.

- `workbench.html` — Option A "Workbench": persistent rail + workspace views.
  **Full-parity mock**: every one of `index.html`'s 209 buttons and 121
  inputs/selects/toggles is present with its real id (remapped real ids like
  `updateCheckBtn2` are marked inline), all 7 modals, the 2×2 launch grid, and
  an Installer view. Elements unique to the redesign carry `.wb-new`/a "new"
  chip (health chips, checklist, profile ladder, pipeline, session card,
  install path fields, Ctrl+K palette).
  The shell uses a mock-only inline SVG icon sprite (`<symbol id="i-*">`) for
  a consistent stroke-icon set; real app markup keeps its own icons.
- **Layout theme demo**: Settings → Appearance → *Layout theme* switches
  between `workbench` (rail + views), `classic` (single scrolling page),
  `deck` (Command Deck / option B: horizontal command strip + live stat
  tiles) and `real08` (the actual current 0.10.8 layout — `../src/index.html`
  loaded read-only in an iframe, its own CSS + tab scripts intact) —
  persisted in `localStorage` as a stand-in for the real
  `layoutTheme` setting in `desktop-config.json`. Both demo shells reuse
  the same ids, so the future `classic | workbench | deck | …` selector in
  the real app can keep every existing tab handler working. A compact
  quick-switcher also lives in the topbar in every layout.
- **Visual theme demo**: topbar theme select (or the ◐/↻ buttons) switches
  Dark · real accents (Sky, Classic, Emerald, Amethyst, Rose — from
  `style.css`) **and mock-only famous themes** — Cyberpunk, Matrix,
  Fantasy, Vaporwave — defined as token-override blocks in the mock file.
  Real adoption would mean moving those blocks into `style.css` and
  registering them in `theme-tab.js`.
