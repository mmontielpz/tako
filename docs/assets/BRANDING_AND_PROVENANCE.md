# TAKO Branding & Asset Provenance

All visual assets in `docs/assets/` are **repository-owned conceptual presentation visuals** derived directly from TAKO's original 16×16 pixel-art companion and design tokens:

- **Source Grid & Colors:** [`ui/src/pixelGrid.ts`](../../ui/src/pixelGrid.ts) (`// 16x16 pixel-art Tako. Drawn for this project.`) and [`src-tauri/icons/icon.png`](../../src-tauri/icons/icon.png).
- **Color & Typography Tokens:** [`docs/product/DESIGN_SYSTEM.md`](../product/DESIGN_SYSTEM.md)
  - Dark canvas: `#14101F`, text: `#E8E4F0`, muted: `#A79FC0`, accent: `#B07CFF`
  - Light canvas: `#F6F3FB`, text: `#1B1530`, muted: `#5E5578`, accent: `#5B2FB8`
  - Octopus palette: `#5B2FB8` (deep purple), `#8B5CF6` (body purple), `#B07CFF` (highlight)
  - Typography: System UI sans-serif + monospace for CLI/telemetry identifiers.
- **Generated Presentation Assets (Conceptual):**
  - `docs/assets/tako-icon.svg` — 16×16 transparent compact pixel-art icon.
  - `docs/assets/tako-logo.svg` — Transparent pixel-art icon + wordmark.
  - `docs/assets/tako-header-dark.svg` — Dark-theme README header banner.
  - `docs/assets/tako-header-light.svg` — Light-theme README header banner.