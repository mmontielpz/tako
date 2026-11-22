# Design System & Workbench Wireframes (ARDE V2.0)

Status: Foundation tokens preserved from `TAKO-001B` / `TAKO-004`, extended with low-fidelity Desktop workbench wireframes for `P7` (`M029E`).

## Direction
Calm, technical, researcher-friendly. Dark theme first, light theme second. Purple (`#B07CFF` / `#5B2FB8`) is the brand accent; pixel-art Research Mate assets are used sparingly; text-heavy reading surfaces stay clean and conventional. Interaction inspiration: GitHub Copilot and Antigravity IDE/ADE (original assets only).

## Color Tokens
| Token | Dark | Light | Notes |
|---|---|---|---|
| `bg` | `#14101F` | `#F6F3FB` | App background |
| `surface` | `#1E1830` | `#FFFFFF` | Panels, cards |
| `text` | `#E8E4F0` | `#1B1530` | 14.9:1 / 16.0:1 on bg |
| `text-muted` | `#A79FC0` | `#5E5578` | 7.4:1 / 6.3:1 on bg |
| `accent` | `#B07CFF` | `#5B2FB8` | 6.4:1 / 7.5:1 on bg |
| `accent-button` | `#6D3FD0` | `#6D3FD0` | White text 6.4:1 |
| `success` | `#5FD38D` | `#1E7E48` | Verified / pass |
| `warning` | `#F2C14E` | `#9A6700` | Candidate / abstract-only |
| `danger` | `#FF6B7A` | `#C92A3E` | Blocked / error |

## Typography & Spacing
- System UI font for interface; monospace stack for CLI logs, BibTeX, and diffs (`12 / 14 / 16 / 20 / 28 px`, minimum body `14 px`).
- 4 px base unit; 6 px control radius; 10 px card radius; visible `accent` focus ring; respect `prefers-reduced-motion` and `NO_COLOR` in CLI.

## Low-Fidelity Desktop Workbench Wireframe
```text
+-----------------------------------------------------------------------------------+
| TAKO ARDE  [Workspace: jetseg-evolution v]  [Task: Create SOTA v]   [Free | AGY]  |
+-------------------+-----------------------------------+---------------------------+
| WORKSPACE         | [SOTA Studio] [Paper: BiSeNet]    | RESEARCH MATE (AGY)       |
| - Library (15)    |-----------------------------------|---------------------------|
| - SOTA Matrix     | Round 2/3 [=====>   ] 12/24 HTTP  | Context: BiSeNet (2018)   |
| - Claims & Gaps   | [x] BiSeNet (VERIFIED, ABSTRACT)  |                           |
| - Notebook        | [x] DDRNet  (VERIFIED, FULL_TEXT) | > Compare BiSeNet vs      |
| - References.bib  | [!] Candidate-48 (UNVERIFIED)     |   DDRNet latency on Orin  |
|                   |                                   |                           |
|                   | [Approve Selection] [Export Bib]  | [Evidence-grounded reply] |
+-------------------+-----------------------------------+---------------------------+
| Status: EXPLORATORY_LITERATURE_MAP | 15/15 Bib Verified | 0 Paid API Calls        |
+-----------------------------------------------------------------------------------+
```

## Simplified Onboarding & Task Selection Wireframe
```text
+---------------------------------------------------------------+
| Welcome to TAKO — Select Research Task                        |
|  [1] Create a State of the Art   [2] Quick Literature Review  |
|  [3] Understand a Paper          [4] Compare Papers           |
|  Setup: Profile=Free | AI CLI=AGY (detected) [Advanced v]     |
+---------------------------------------------------------------+
```
