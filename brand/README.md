# anymd brand

## Shared generator

CI uses the shared brand action pinned to `a6c81b4bcda66bf624f0a68e25e63b3c0ed043eb`.
The masters, tokens, pixel grids and provenance remain in this repository;
existing assets are unchanged by moving the generator. From the repository
root, run this self-contained recipe. Select `OPERATION=write` to regenerate
and verify, `OPERATION=check` to verify only, or `OPERATION=resnap` to
intentionally redraw the small favicon grids, regenerate and verify.

```sh
(
  set -eu
  OPERATION=write # Choose write, check or resnap before running.
  BRAND_SCRIPT="$(mktemp)"
  trap 'rm -f "$BRAND_SCRIPT"' EXIT
  curl --fail --location --output "$BRAND_SCRIPT" \
    "https://raw.githubusercontent.com/SylphxAI/.github/a6c81b4bcda66bf624f0a68e25e63b3c0ed043eb/.github/actions/brand/build.py"
  case "$OPERATION" in
    check) set -- --check ;;
    write) set -- ;;
    resnap) set -- --resnap ;;
    *) echo "Unknown brand operation: $OPERATION" >&2; exit 1 ;;
  esac
  if [ "$OPERATION" != check ]; then
    python3 -m pip install pillow numpy resvg-py
  fi
  python3 "$BRAND_SCRIPT" --brand-dir "$PWD/brand" --root "$PWD" "$@"
  if [ "$OPERATION" != check ]; then
    python3 "$BRAND_SCRIPT" --brand-dir "$PWD/brand" --root "$PWD" --check
  fi
)
```

Check mode needs only Python 3 and does not regenerate files. Each invocation
prepares and cleans up its own script; later references select an operation in
this recipe, rather than reusing its temporary path. A download, dependency,
regeneration or verification failure stops the recipe with a nonzero status.
Generated-file comments that name `brand/build.py` describe the historical
generator; they are preserved to keep the asset bytes and hashes unchanged.

This folder is the source of truth for the anymd brand: every surface is a copy of a file here, so a change lands in a master or in `tokens.json`, and the [shared recipe](#shared-generator) with `OPERATION=write` writes the generated files and refreshes the surface copies (it needs Pillow, numpy and resvg-py). CI runs the [shared recipe](#shared-generator) with `OPERATION=check`, which needs only Python 3.

## Name

- Written `anymd`, one word, all lower case — including as the first word of a sentence ("anymd turns any file into Markdown for AI agents", `AGENTS.md`) and in headings ("## Why anymd", `README.md`). No capital is used anywhere the name is spelled.
- All caps appear only inside environment variable names (`ANYMD_BIN`, `ANYMD_CACHE_DIR`, `docs/guide/formats.md`); `Anymd` appears only as the name of an SDK class removed in 8.0.0 (`docs/guide/migration.md`).
- The mark carries no letters, so there is no capitalised form inside the logo.
- Former names: Citra and pdf-reader-mcp (`AGENTS.md`, `docs/vision.md`). The old repository slugs redirect here, and `@sylphx/citra` and `@sylphx/pdf-reader-mcp` remain as compatibility alias packages (`packages/aliases/`).
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
| the generator | [shared generator](https://github.com/SylphxAI/.github/tree/a6c81b4bcda66bf624f0a68e25e63b3c0ed043eb/.github/actions/brand) |

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
| `brand-text` | `#3F6100` | links and accent text on the light ground; docs `--vp-c-brand-1` |
| `brand-text-hover` | `#4A7000` | the same, hovered; docs `--vp-c-brand-2` |
| `brand-solid` | `#82B800` | borders and solid accents on the light ground; docs `--vp-c-brand-3` |
| `brand-solid-dark` | `#86B321` | the ramp's darkest step on the dark ground; docs `--vp-c-brand-3` in the dark theme |

The docs site reads them through `docs/.vitepress/theme/custom.css`, which imports `brand/tokens.css` and sets each theme variable to `var(--brand-color-<name>)`.

## Type

- Interface and body: the reader's own system faces (`--brand-font-sans`).
- Code, keys and the terminal demo: the system mono (`--brand-font-mono`).
- The docs site applies both, so a page loads no external font, script or image.
- The social card is the exception: `og/anymd-og.html` loads Inter and JetBrains Mono from Google Fonts while it is open in a browser (both under the SIL Open Font License). What ships is the rendered PNG, and this repository hosts no font file.

## Small sizes

The 16 and 32 px favicons are drawn from `favicon/grid-16.txt` and `favicon/grid-32.txt`, not straight from the vector. Each character is one pixel: `.` is empty, `A` is `#C3F53C`, `B` is `#0A0D0A`, `C` is `#2B3A05`. The grid files can be hand-edited, and build.py draws from the edited file until the [shared recipe](#shared-generator) with `OPERATION=resnap` redraws both from the master at 8× and snaps every pixel to the nearest colour in `icon.palette`. The third evidence line is drawn at 55% opacity over the document; its blend lands nearest the fold green, so the grids show it as `C`.

## Clear space and minimum size

Keep the mark legible at 16 px, the floor the drawings are checked at: `favicon/grid-16.txt` is the hand-checked drawing the favicon set is generated from.

## Do / Don't

What this folder enforces by construction:

- Do change a colour once, in `tokens.json`; [shared generator](https://github.com/SylphxAI/.github/tree/a6c81b4bcda66bf624f0a68e25e63b3c0ed043eb/.github/actions/brand) writes `tokens.css`, and the surfaces read it.
- Do use `svg/anymd-symbol-black.svg` or `-white.svg` when one colour is needed.
- Don't edit a generated file (`favicon/`, `app-icon/`, `tokens.css`, `provenance.json`, or any surface copy) — the next build overwrites it.
- Don't redraw the mark from scratch; edit a master, then run [shared generator](https://github.com/SylphxAI/.github/tree/a6c81b4bcda66bf624f0a68e25e63b3c0ed043eb/.github/actions/brand).

## Surfaces

| Surface | Copies |
| --- | --- |
| `docs/public/logo.svg` — the docs site logo, the home hero image, and the URL used by other pages | `svg/anymd-app-icon.svg` |
| `docs/public/favicon.svg` — the docs site icon | `favicon/favicon.svg` |
| `docs/public/favicon.ico` — the fallback icon | `favicon/favicon.ico` |
| `docs/public/og-image.png` — `og:image`, `twitter:image`, and the README hero | `og/anymd-og.png` |
| `docs/public/og-image.svg` — still served, linked by nothing | `og/anymd-og-vector.svg` |

## Provenance

Everything under `favicon/` and `app-icon/`, plus `tokens.css`, is generated by [shared generator](https://github.com/SylphxAI/.github/tree/a6c81b4bcda66bf624f0a68e25e63b3c0ed043eb/.github/actions/brand); `brand.json` is the spec that says what feeds what. Every file's SHA-256 is in `provenance.json`.

## Trademark

Not registered. Owner decision owner#781: no trademark filings before the product earns money. Use ™ at most, never ®.

**Same name, same category, several times over:** muthuishere/anymd (Go document-to-Markdown library and CLI), Ljf857/anymd (Python MCP server converting files to Markdown), digitopvn/anymd (anymd.cc, web-to-Markdown service with API, CLI and MCP). All are open-source converters doing the job ours does. Search results and MCP directories will mix them. Noted once, not blocking; the scoped package names (@sylphx/anymd) and the SylphxAI org carry the distinction. Mark (a document on a lime tile): no close match found.
