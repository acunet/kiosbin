# Warkop 3Dara POS (RustPOS)

A single-binary point of sale for one small Indonesian coffee shop, replacing a full
Frappe v15 + ERPNext v15 + POSNext deployment that was far too heavy for 3–11 sales a day
(±23k lines of POS Python, ±60k lines of frontend, 113 API endpoints, 13 DocTypes just for
shifts and offers).

What is kept from the ERP world: a **stock ledger for raw materials** driven by recipes.
What is dropped: accounting/GL, taxes, payroll, MRP, wallet/loyalty, promotions, credit sales,
table service, multi-warehouse, batches/serials.

## Stack

Rust · `axum` (web, landing soon) · SQLite via `sqlx` (WAL) · HTMX + Askama (server-rendered,
no npm, no build step) · ESC/POS over TCP for the thermal printer.

## Quickstart

```bash
export PATH="$HOME/.cargo/bin:$PATH"

cargo run -- migrate                        # create/upgrade warkop.db
cargo run -- seed seed/warkop_seed.json     # catalogue snapshot: 39 menu items + 44 ingredients
cargo run -- check                          # counts, items without a price, ingredients at/below zero
cargo test                                  # money, migration, constraints, seed idempotency
```

`--db PATH` (or `WARKOP_DB`) points at a different database file.

Expected seed output:

```text
seed: ok  items written=83 kept=0  recipe lines=70  uom conversions added=4  settings added=8
seed: ok  items written=0 kept=83  recipe lines=0  uom conversions added=0  settings added=0   # re-run: nothing duplicated
```

## Rules the code is built around

- Money is always integer rupiah (`src/money.rs`) — no floats anywhere near money.
- Stock balance is never stored: it is `SUM(stock_moves.qty)`.
- Only raw materials (and paid add-ons) are stock-tracked — menu items are not.
- Cash reconciliation is typed by the cashier; `counted` is never auto-filled from `expected`.
- Printing failures never block a sale, and every print attempt is logged.

`seed` leaves existing rows alone so a price edited in the POS is not silently reverted;
`seed --force` re-reads the catalogue when the source really changed.

## Status

Step 1–2 of 12 (schema + migrate + seed) are done and verified. See `AGENTS.md` §0 for the
full ordered plan, the open decisions, and the till UX rules.

`AGENTS.md` is written in Indonesian (internal ops doc); code, comments, commits and this file
are English. The business reference implementation (the old ERPNext system) lives in the
sibling repo `warkop3dara.id` and is never part of this build.

## License

AGPL-3.0-or-later.
