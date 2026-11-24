# DECISIONS — Kiosbin

Recorded so they do not get relitigated. Each is reversible, but only on purpose.

| # | Date | Decision | Why |
|---|---|---|---|
| D1 | 2026-11-24 | **One repository, one binary**: backend + UI in the same crate, HTMX/Askama server-rendered | No build step, no API version skew, no stale bundle, one file to install. The HTTP boundary is the seam if a native client ever needs splitting off. |
| D2 | 2026-11-24 | **SQLite (WAL) is the database**, not "SQLite for now, Postgres later" | Single file = backup is a file copy, zero DB administration for a shop with 3–11 receipts a day. It is also a selling point for self-hosting. Postgres/multi-outlet is only worth it if a paying customer needs several outlets behind one server. |
| D3 | 2026-11-24 | Timestamps come from the database through `db::NOW_SQL` / `db::now()` — the single dialect seam | One clock for every row, and when a second database is ever added there is exactly one place using SQLite date functions. |
| D4 | 2026-11-24 | Everything shop-specific lives in a **fixture** file, never in code | The product must sell to the next shop without a rebuild. The binary contains no shop name, price, printer IP or currency. |
| D5 | 2026-11-24 | Item `kind` comes from the fixture; `infer_kind()` is only a fallback for hand-written files | Prefix conventions (`BB-`, `MN-AD-`) are one tenant's habit, not a product rule. |
| D6 | 2026-11-24 | Import is **additive**: existing rows (prices, settings) are never overwritten; `--force` does that deliberately | An import that silently reverts a price the owner changed is worse than no import. |
| D7 | 2026-11-24 | Recipe pointing at an unknown ingredient **fails the whole import and rolls back** | Silently skipping it = stock never deducts and margin reads 0 forever (this already happened in the ERP install). |
| D8 | 2026-11-24 | Vouchers (WiFi/access codes) are a first-class product feature, voucher issuing *providers* are not | Selling vouchers is generic small-shop business; MikroTik is one provider, so only the generic columns and a `provider_ref` live in the schema. |
| D9 | 2026-11-24 | Menu items are **not** stock-tracked; only ingredients (and paid add-ons with a recipe) are | The ERP install tracked menus as stock and produced 17 negative stock rows nobody read. |

## Open decisions

| # | Question | Blocks |
|---|---|---|
| O1 | **Licence** for a product you intend to sell: proprietary, or open-core with a commercial edition? Dependency licences must be audited first (sqlx/axum/askama are permissive; verify the vendored HTMX and any future asset). The old ERP app being AGPL is irrelevant — this is a new codebase. | Any distribution or public repo |
| O2 | Receipt paper width: 58 mm (32 cols) or 80 mm (48 cols)? Fixture default is 48. | Printing step |
| O3 | Voucher provider support in v1 (which provider, credentials handling) or leave `vouchers` empty for now? | Voucher step |
| O4 | Negative stock: block the sale, or allow and flag it? | Till payment step |
| O5 | Master-data editing: CSV/CLI import only, or an owner web page for menu and prices? | Before any real deployment |
| O6 | Update channel for sold copies: manual binary copy, or a signed release/`kiosbin upgrade`? | Before selling |
