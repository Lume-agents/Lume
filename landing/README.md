# Lume landing

Standalone product presentation using Vite, HTML, CSS and JavaScript.

## Run and build

- `npm run landing:dev` → http://127.0.0.1:4174
- `npm run landing:build` → static output in `landing-dist/`
- Set `LANDING_SITE_URL` at build time to supply canonical and absolute social-image URLs.

## Presentation

The page presents two complete modes: Orb with independent floating terminals, and Workspace. Product images are captures of the actual Svelte components with illustrative sessions. The page has no simulated agent controls.

The opening sculpture uses Three.js, instanced geometry, physical materials and environment lighting. Native scrolling opens the Orb composition; subsequent motion supports the product images and architecture. Rendering stops when settled and pauses offscreen. Reduced-motion, data-saving and unavailable-WebGL visitors receive the static composition. Text and links remain usable without JavaScript.

GitHub/Jira and distributed execution are identified as in development according to the checkout documentation. Node phase one is described as identity, pairing and read-only health.

## Assets and documentation

Fonts are self-hosted. Their licenses and the Three.js license ship under `licenses/`. Product PNGs carry their source provenance. Third-party reference images and review captures do not ship.

- `DIRECTION.md`: user correction, visual direction, references and feature truth.
- `DESIGN.md` and `.impeccable/design.json`: implemented visual system.
- `assets/product/`: current Orb, expanded panel, floating terminals and Workspace captures.

Before publishing, confirm feature availability against the linked release. The static build does not include the native app or any capture fixture.
