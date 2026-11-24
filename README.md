# RustPOS

A self-hosted point of sale for small shops, cafés and warungs — **one binary, one SQLite
file, no cloud and no npm**. It does the two jobs a small outlet actually needs: take
payment at the till, and keep an honest stock ledger of the ingredients behind every sale.

Built to replace installs where a full ERP (Frappe/ERPNext + a Vue POS: ±23k lines of POS
Python, ±60k lines of frontend, 113 API endpoints) was doing the work of 3–11 receipts a day.

## Why one binary instead of a service + frontend

The UI is server-rendered (HTMX + Askama templates compiled into the binary), so there is no
second build, no API version skew, no CORS, no stale JS bundle and nothing to install on the
till besides opening a browser. Deploying = copying one file and starting one systemd unit.
If a native/mobile client is ever needed, the HTTP boundary is already the seam to split on.

## What it does

- **Till**: item grid + search by code/name, cart, quantity, paid add-ons as their own lines,
  per-line notes, cash with change, static QRIS, invoice-level discount.
- **Shifts**: open with a float, per-method expected vs counted, EOD report. `counted` is
  typed by the cashier and never auto-filled from `expected` — that is what keeps cash
  control alive.
- **Stock ledger**: recipes deduct ingredients on submit, `SUM(stock_moves.qty)` is the
  balance (never stored), opening stock, stock-takes, waste, purchases (cost → margin).
- **Vouchers**: service items (WiFi/access vouchers) can issue a code per unit and print it
  on the receipt.
- **Owner reports** that open on a phone, plus CSV.

## Quickstart

```bash
export PATH="$HOME/.cargo/bin:$PATH"

cargo run -- migrate                                  # create/upgrade rustpos.db
cargo run -- import fixtures/warkop3dara.seed.json     # load an outlet fixture
cargo run -- check                                    # counts, items without a price, negative stock
cargo test                                            # 13 tests
```

Expected import output:

```text
import: ok  items written=83 kept=0  recipe lines=70  uom conversions added=4  settings added=14
import: ok  items written=0 kept=83  recipe lines=0  uom conversions added=0  settings added=0   # re-import: nothing duplicated
```

`--db PATH` (or `RUSTPOS_DB`) points at another database file. `import --force` re-reads the
catalog from the fixture when the source changed; a plain import never overwrites rows an
outlet has edited.

## Tenants

Everything shop-specific lives in a **fixture** (`fixtures/*.seed.json`): outlet name,
currency, invoice prefix, printer target, payment methods, UOM conversions, catalog, recipes.
The binary has no shop name, price, printer IP or currency in it. `fixtures/warkop3dara.seed.json`
is the reference tenant used by the tests and as the first real deployment.

## Rules the code is built around

- Money is an integer in the currency's minor unit (`src/money.rs`) — no floats near money.
- A stock balance is never stored; it is `SUM(stock_moves.qty)`.
- Only ingredients (and paid add-ons with a recipe) are stock-tracked; menu items are not.
- Sales are voided, never deleted: status + reversing moves, invoice numbers never reused.
- One open shift per cashier, enforced by a unique index in the database.
- Print failures never block a sale, and every attempt is logged.

## Status

Schema, migrate and catalog import (steps 1–2 of the plan) are done and verified on Windows
and Linux (WSL Ubuntu 24.04): `cargo test` green, `clippy -D warnings` clean, and a real
`migrate → import → check` run against a fresh database.

Next: PIN/shift opening, then the till grid and payment. See `AGENTS.md` for the ordered plan,
the till UX rules and the open decisions.
