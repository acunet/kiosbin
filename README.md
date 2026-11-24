# Kiosbin

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

## Try it (no install)

```bash
export PATH="$HOME/.cargo/bin:$PATH"

cargo run -- migrate                                    # create kiosbin.db
cargo run -- import fixtures/warkop3dara.seed.json       # reference tenant catalog
printf '1234\n' | cargo run -- user add Rina             # a cashier with pin 1234
cargo run -- serve --bind 127.0.0.1:8080                 # the till, same binary
```

Open <http://localhost:8080> (or `http://<pc-ip>:8080` from a tablet on the same WiFi):
log in with the PIN, enter the opening float, tap items, pay cash with change, read the
receipt (including the ESC/POS preview of what a thermal printer would receive), then close
the shift by typing the physical cash count. `ops/smoke.sh` drives that whole path over HTTP.

## What it does

- **Till**: item grid + search by code/name, cart, quantity, paid add-ons as their own lines,
  per-line notes, cash with change, static QRIS, invoice-level discount.
- **Shifts**: open with a float, per-method expected vs counted, EOD report. `counted` is
  typed by the cashier and never auto-filled from `expected` — that is what keeps cash
  control alive.
- **Stock ledger**: recipes deduct ingredients on submit, `SUM(stock_moves.qty)` is the
  balance (never stored), opening stock, stock-takes, waste, purchases (cost → margin).
- **Vouchers**: service items (WiFi/access vouchers) can issue a code per unit and print it
  on the receipt (schema ready, provider integration is a later step).
- **Owner reports** that open on a phone, plus CSV (next step).

## Quickstart

```bash
export PATH="$HOME/.cargo/bin:$PATH"

cargo run -- migrate                                  # create/upgrade kiosbin.db
cargo run -- import fixtures/warkop3dara.seed.json     # load an outlet fixture
cargo run -- check                                    # counts, items without a price, negative stock
cargo test                                            # 36 tests
```

Expected import output:

```text
import: ok  items written=83 kept=0  recipe lines=70  uom conversions added=4  settings added=16
import: ok  items written=0 kept=83  recipe lines=0  uom conversions added=0  settings added=0   # re-import: nothing duplicated
```

`--db PATH` (or `KIOSBIN_DB`) points at another database file. `import --force` re-reads the
catalog from the fixture when the source changed; a plain import never overwrites rows an
outlet has edited.

## Languages and money formats

The till, the shift screens and the printed receipt follow the outlet's `language` setting:
`"id"` (Indonesian) or `"en"` (English). It comes from the fixture, or from the `settings` table,
and it is read per request — switching it does not need a restart. An outlet that names no
language gets English.

Language and money are separate settings, on purpose: `currency_symbol`, `currency_decimals` and
`currency_group_sep` decide how money prints (`Rp 1.250.000` vs `$ 1,250,000.00`), so an English
shop in a country that groups with commas says so once and the language is irrelevant to it.

**Every string a person reads lives in `src/i18n.rs`** — one table with both languages side by
side. Adding a string is one row and the code will not compile until both sides of that row are
filled in; adding a language is one more column. Templates and handlers contain no text of their
own, so a third language costs one file and no template edits.

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
- Text a person reads only ever comes from `src/i18n.rs`; money formats from the currency settings.
- One open shift per cashier, enforced by a unique index in the database.
- Print failures never block a sale, and every attempt is logged.

## Status

Works today (steps 1–6 of the plan): schema, migrate, catalog import, PIN login, shift
open/close with reconciliation, the till (grid, cart, cash/QRIS payment) and the receipt
layout. 30 tests, green on Windows and Linux, plus a real localhost run driven over HTTP
(`ops/smoke.sh`).

Next: printing to the thermal printer, opening-stock screen, owner reports, master-data
editing. See `AGENTS.md` for the ordered plan, the till UX rules and the open decisions.
