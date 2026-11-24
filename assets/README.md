# Vendored assets

| File | Version | License | Source |
|---|---|---|---|
| `htmx.min.js` | 2.0.11 | 0BSD (permissive, no attribution required) | `https://unpkg.com/htmx.org@2/dist/htmx.min.js` |

HTMX is embedded into the binary with `include_str!` and served at `/assets/htmx.min.js`, so an
installed shop has no CDN dependency and still ships as a single file. Version and license were
read from the package's own `package.json`; re-check them when upgrading.
