# Brand sources

Verbatim copies of the Redrob design system's shipped files (`redrob-design-system/assets/`).
`SHA256SUMS` pins them, and `packaging/brand-assets.sh` derives everything under
`assets/app-icon/` from these and nothing else.

| Files | From | Used for |
|---|---|---|
| `favicon-*.png`, `icon-192.png`, `icon-512.png`, `favicon.ico`, `favicon.svg` | `Favicons/` | Redrob Board's app icon (the Redrob tile: Board has no product mark of its own yet) |
| `symbol-white-*.png` | `Favicons/` | The symbol alone on transparent, for a dark ground that is not Redrob Blue |
| `redrob-logo-primary.png`, `redrob-logo-primary-white.png` | `Logos/` | The header lockup: dark wordmark on light grounds, white wordmark on dark grounds |

Rules that bind here (design system `10-logo.md`, `Logos/README.md`, `Products/README.md`):

- A white symbol on a Redrob Blue (`#2B52FF`) tile. One design, no dark-mode variant.
- Never redrawn, recolored, re-proportioned, given a shadow, border or second radius. Never a
  macOS template image: the system would recolor it.
- Each size is drawn for itself. Where the design system ships a size it is used as is; any other
  size is the next larger file resized down. Nothing is upscaled, so no icon here is larger than 512.
- The lockup variant is picked by ground; it is never tinted to fit one.

Redrob Cut, Shape, CAD, Canvas, Studio, IDE, Cowork, Query and Recall have no product mark in the
design system yet, so they carry the Redrob tile. When a mark is added to `Products/`, copy its SVG
here, add it to `SHA256SUMS` and to `packaging/brand-assets.sh`, and rerun the script.
