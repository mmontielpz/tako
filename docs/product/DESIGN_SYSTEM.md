# Design System (foundation)

Status: starting tokens for TAKO-004. Small by intent. Contrast ratios below were computed (WCAG relative-luminance formula) during TAKO-001B; they cover only these pairs.

## Direction

Calm, technical, friendly. Dark theme first, light theme second. Purple is the brand accent; pixel-art is used for the companion and sparingly for icons, not for body UI. Text-heavy surfaces stay clean and conventional.

## Color tokens

| Token | Dark | Light | Notes |
| --- | --- | --- | --- |
| `bg` | `#14101F` | `#F6F3FB` | app background |
| `surface` | `#1E1830` | `#FFFFFF` | panels, cards |
| `text` | `#E8E4F0` | `#1B1530` | 14.9:1 / 16.0:1 on bg |
| `text-muted` | `#A79FC0` | `#5E5578` | 7.4:1 / 6.3:1 on bg |
| `accent` | `#B07CFF` | `#5B2FB8` | 6.4:1 / 7.5:1 on bg |
| `accent-button` | `#6D3FD0` | `#6D3FD0` | white text 6.4:1 |
| `success` | `#5FD38D` | tbd | 10.0:1 on dark bg |
| `warning` | `#F2C14E` | tbd | 11.1:1 on dark bg |
| `danger` | `#FF6B7A` | tbd | 6.8:1 on dark bg |

Light-theme status colors are unspecified and must be validated before use. Status is always conveyed by icon + text as well as color.

## Typography

System UI font for interface; a monospace stack for code, diffs and logs. Scale: 12 / 14 / 16 / 20 / 28 px. No custom font dependency in the MVP.

## Spacing and shape

4 px base unit (4, 8, 12, 16, 24, 32). Radius 6 px for controls, 10 px for cards. Pixel-art assets scale only by integers with `image-rendering: pixelated`.

## Components needed first

State chip, button (primary/secondary/destructive), checklist (permissions), diff viewer (read-only), log viewer (collapsible), evidence card, banner (info/warn/error), companion panel.

## Rules

Visible focus ring in `accent`; respect `prefers-reduced-motion`; minimum body text 14 px; no information by color alone.
