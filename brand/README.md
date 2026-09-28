# anymd brand

This folder is the source of truth for the anymd brand: every surface is a copy of a file here, so a change lands in a master or in `tokens.json`, and `python3 brand/build.py` writes the generated files and refreshes the surface copies (it needs Pillow, numpy and resvg-py). CI runs `python3 brand/build.py --check`, which needs only Python 3.

## Name

- Written `anymd`, one word, all lower case — including as the first word of a sentence ("anymd turns any file into Markdown for AI agents", `AGENTS.md`) and in headings ("## Why anymd", `README.md`). No capital is used anywhere the name is spelled.
- All caps appear only inside environment variable names (`ANYMD_BIN`, `ANYMD_CACHE_DIR`, `docs/guide/formats.md`); `Anymd` appears only as the name of an SDK class removed in 8.0.0 (`docs/guide/migration.md`).
- The mark carries no letters, so there is no capitalised form inside the logo.
- Former names: Citra and pdf-reader-mcp (`AGENTS.md`, `PROJECT.md`). The old repository slugs redirect here, and `@sylphx/citra` and `@sylphx/pdf-reader-mcp` remain as compatibility alias packages (`packages/aliases/`).
- Local-script names: none; the docs are English only.
- Operator line: the repository's docs do not carry one yet. Its licence reads `Copyright (c) 2024-2026 SylphxAI` and the site footer `Copyright 2024–2026 Sylphx`. An operator line reads "operated by Sylphx Limited" and is never part of the brand.

## Files

| Need | File |
| --- | --- |
| the mark as shipped (citrus tile, document, citation pip) | `svg/anymd-app-icon.svg` |
| the document alone, on a tight box | `svg/anymd-symbol.svg` |
| the symbol in one colour | `svg/anymd-symbol-black.svg`, `svg/anymd-symbol-white.svg` |
| the icon squared off, for iOS | `svg/anymd-app-icon-square.svg` |
| the icon for launcher masks | `svg/anymd-maskable.svg` |
| the social card, 1280×640 | `og/anymd-og.png` |
| the social card's source (open in a browser, screenshot) | `og/anymd-og.html` |
| the earlier all-vector social card, still served | `og/anymd-og-vector.svg` |
| favicons | `favicon/favicon.svg`, `favicon/favicon.ico`, `favicon/favicon-16.png`, `favicon/favicon-32.png`, `favicon/favicon-48.png` |
| the 16 and 32 px drawings, hand-editable | `favicon/grid-16.txt`, `favicon/grid-32.txt` |
| app icons | `app-icon/apple-touch-icon-180.png`, `app-icon/icon-192.png`, `app-icon/icon-512.png`, `app-icon/icon-1024.png`, `app-icon/icon-maskable-192.png`, `app-icon/icon-maskable-512.png` |
| colours | `tokens.json`, generated to `tokens.css` |
| what feeds what, and where each file came from | `brand.json`, `provenance.json` |
| the generator | `build.py` |

## Colours

| Token | Hex | Use |
| --- | --- | --- |
| `citrus` | `#C3F53C` | the mark's tile; the accent; docs `--vp-c-brand-1` and `-2` in the dark theme |
| `citrus-deep` | `#9FD21C` | the accent pressed or hovered |
| `citrus-ink` | `#243503` | text and icons on a citrus ground |
| `paper` | `#FBFCF8` | page ground; light theme |
| `ink` | `#0A0D07` | page ground; dark theme, the docs default |
| `mark-ink` | `#0A0D0A` | the document in the mark |
| `mark-fold` | `#2B3A05` | the folded corner in the mark |
| `brand-text` | `#5C8A00` | links and accent text on the light ground; docs `--vp-c-brand-1` |
| `brand-text-hover` | `#6FA000` | the same, hovered; docs `--vp-c-brand-2` |
| `brand-solid` | `#82B800` | borders and solid accents on the light ground; docs `--vp-c-brand-3` |
| `brand-solid-dark` | `#86B321` | the ramp's darkest step on the dark ground; docs `--vp-c-brand-3` in the dark theme |

The docs site reads them through `docs/.vitepress/theme/custom.css`, which imports `brand/tokens.css` and sets each theme variable to `var(--brand-color-<name>)`.

## Type

- Interface and body: the reader's own system faces (`--brand-font-sans`).
- Code, keys and the terminal demo: the system mono (`--brand-font-mono`).
- The docs site applies both, so a page loads no external font, script or image.
- The social card is the exception: `og/anymd-og.html` loads Inter and JetBrains Mono from Google Fonts while it is open in a browser (both under the SIL Open Font License). What ships is the rendered PNG, and this repository hosts no font file.

## Small sizes

The 16 and 32 px favicons are drawn from `favicon/grid-16.txt` and `favicon/grid-32.txt`, not straight from the vector. Each character is one pixel: `.` is empty, `A` is `#C3F53C`, `B` is `#0A0D0A`, `C` is `#2B3A05`. The grid files can be hand-edited, and build.py draws from the edited file until `python3 brand/build.py --resnap` redraws both from the master at 8× and snaps every pixel to the nearest colour in `icon.palette`. The third evidence line is drawn at 55% opacity over the document; its blend lands nearest the fold green, so the grids show it as `C`.

## Clear space and minimum size

Not yet specified — this repository has no design doc that sets clear space or a minimum size. The floor the drawings are checked at is 16 px: `favicon/grid-16.txt` is the hand-checked drawing the favicon set is generated from.

## Do / Don't

No design doc in this repository carries do / don't rules yet. What this folder enforces by construction:

- Do change a colour once, in `tokens.json`; `build.py` writes `tokens.css`, and the surfaces read it.
- Do use `svg/anymd-symbol-black.svg` or `-white.svg` when one colour is needed.
- Don't edit a generated file (`favicon/`, `app-icon/`, `tokens.css`, `provenance.json`, or any surface copy) — the next build overwrites it.
- Don't redraw the mark from scratch; edit a master, then run `build.py`.

## Surfaces

| Surface | Copies |
| --- | --- |
| `docs/public/logo.svg` — the docs site logo, the home hero image, and the URL used by other pages | `svg/anymd-app-icon.svg` |
| `docs/public/favicon.svg` — the docs site icon | `favicon/favicon.svg` |
| `docs/public/favicon.ico` — the fallback icon | `favicon/favicon.ico` |
| `docs/public/og-image.png` — `og:image`, `twitter:image`, and the README hero | `og/anymd-og.png` |
| `docs/public/og-image.svg` — still served, linked by nothing | `og/anymd-og-vector.svg` |

Surfaces still to move:

- `README.md` — the five badges hard-code `color=c3f53c` and `labelColor=0a0d07` in their URLs.
- `docs/.vitepress/config.ts` — the `theme-color` meta is the literal `#c3f53c`; a meta tag cannot read a CSS variable.
- `docs/.vitepress/theme/custom.css` — the alpha tints are still literal `rgba(195, 245, 60, …)`, `rgba(122, 176, 8, …)`, `rgba(159, 210, 28, …)` and `rgba(10, 13, 7, …)`; every solid colour comes from `tokens.css`.
- `brand/og/anymd-og.html` — embeds the mark as a hand-refreshed `data:` URI and carries its own colour values; nothing wires it to the master.

## Provenance

- `svg/anymd-app-icon.svg` — `docs/public/logo.svg`: added in 426bb5c (2025-04-06), redrawn in a06881d (2026-09-22, #726), redrawn for the anymd name in 4d7adfd (2026-09-25, #741); moved here on 2026-09-28 with the "Citra mark" comment corrected.
- `svg/anymd-symbol.svg`, `svg/anymd-symbol-black.svg`, `svg/anymd-symbol-white.svg`, `svg/anymd-app-icon-square.svg`, `svg/anymd-maskable.svg` — derived from the app-icon master on 2026-09-28.
- `og/anymd-og.png` — a browser screenshot of `og/anymd-og.html` at 1280×640; the page was added in 435437f (2026-09-25, #754) as `bench/demo/og-image.html` and served at `docs/public/og-image.png`.
- `og/anymd-og.html` — moved from `bench/demo/og-image.html` on 2026-09-28; the logo it inlines as a data URI was refreshed to the current master then.
- `og/anymd-og-vector.svg` — `docs/public/og-image.svg`: added in 1372688 (2026-07-01, #340), redrawn in a06881d (2026-09-22, #726) and 4d7adfd (2026-09-25, #741).
- `tokens.json` — the colours of the "citrus lab" theme (`docs/.vitepress/theme/custom.css`, added in ed1d152, 2025-12-17) and of the app-icon master, written out on 2026-09-28.
- Everything under `favicon/` and `app-icon/`, plus `tokens.css`, is generated by `build.py`; `brand.json` is the spec that says what feeds what.

Every file's SHA-256 is in `provenance.json`.

## Trademark

Not registered. Owner decision owner#781: no trademark filings before the product earns money. Use ™ at most, never ®.

<!-- similarity: filled in by review -->
