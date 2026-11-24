-- 0001_init.sql — Warkop 3Dara POS (RustPOS) initial schema.
--
-- Rules that the schema itself enforces:
--   * money is INTEGER rupiah — never REAL (docs/ASSESSMENT §6.8 of the old repo)
--   * quantities are REAL only for ingredients weighed/counted in Gram / Ml
--   * stock balance is never stored — it is SUM(stock_moves.qty)
--   * a sale is voided by status + reversing moves, never deleted

CREATE TABLE users (
    id       INTEGER PRIMARY KEY,
    name     TEXT    NOT NULL UNIQUE,
    pin_salt TEXT    NOT NULL,
    pin_hash TEXT    NOT NULL,
    role     TEXT    NOT NULL CHECK (role IN ('kasir', 'owner')),
    active   INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE items (
    id         INTEGER PRIMARY KEY,
    code       TEXT    NOT NULL UNIQUE,
    name       TEXT    NOT NULL,
    group_name TEXT    NOT NULL DEFAULT '',
    price      INTEGER NOT NULL DEFAULT 0,  -- rupiah; 0 = till must ask for a price
    uom        TEXT    NOT NULL DEFAULT 'Portion',
    kind       TEXT    NOT NULL CHECK (kind IN ('menu', 'addon', 'raw', 'service')),
    active     INTEGER NOT NULL DEFAULT 1,
    cost       INTEGER NOT NULL DEFAULT 0,  -- HPP per unit; 0 = not filled in yet
    -- MikroTik hotspot voucher config (carried over from the ERPNext item custom fields).
    -- hotspot_enabled = 1 turns the item into "this sale issues a voucher".
    hotspot_enabled          INTEGER NOT NULL DEFAULT 0,
    mikrotik_profile         TEXT,
    hotspot_duration_minutes INTEGER,
    hotspot_validity_minutes INTEGER
);

CREATE INDEX idx_items_kind ON items (kind);

-- Only non-trivial pairs are stored: from_uom = to_uom is factor 1 by definition.
CREATE TABLE uom_conversions (
    from_uom TEXT NOT NULL,
    to_uom   TEXT NOT NULL,
    factor   REAL NOT NULL CHECK (factor > 0),
    PRIMARY KEY (from_uom, to_uom)
);

CREATE TABLE recipes (
    item_id       INTEGER NOT NULL REFERENCES items (id),
    ingredient_id INTEGER NOT NULL REFERENCES items (id),
    qty           REAL    NOT NULL CHECK (qty > 0),  -- per serving, in the ingredient's base uom
    uom           TEXT    NOT NULL,
    PRIMARY KEY (item_id, ingredient_id)
);

CREATE TABLE shifts (
    id            INTEGER PRIMARY KEY,
    cashier       TEXT    NOT NULL,
    opened_at     TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
    closed_at     TEXT,
    opening_float INTEGER NOT NULL DEFAULT 0,
    status        TEXT    NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed'))
);

-- One open shift per cashier (double-open guard still holds without the app running).
CREATE UNIQUE INDEX idx_shifts_one_open ON shifts (cashier) WHERE status = 'open';

CREATE TABLE sales (
    id          INTEGER PRIMARY KEY,
    number      TEXT    NOT NULL UNIQUE,  -- W3D-YYYYMMDD-NNN
    shift_id    INTEGER NOT NULL REFERENCES shifts (id),
    cashier     TEXT    NOT NULL,
    at          TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
    subtotal    INTEGER NOT NULL,
    discount    INTEGER NOT NULL DEFAULT 0,
    total       INTEGER NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'paid' CHECK (status IN ('paid', 'void')),
    legacy_ref  TEXT,  -- ACC-SINV-… from ERPNext, kept for the parallel-run period
    deducted_at TEXT   -- set when this sale's stock_moves were written (idempotency stamp)
);

CREATE INDEX idx_sales_at ON sales (at);
CREATE INDEX idx_sales_shift ON sales (shift_id);

CREATE TABLE sale_lines (
    id             INTEGER PRIMARY KEY,
    sale_id        INTEGER NOT NULL REFERENCES sales (id),
    item_id        INTEGER NOT NULL REFERENCES items (id),
    name           TEXT    NOT NULL,
    qty            INTEGER NOT NULL CHECK (qty > 0),
    unit_price     INTEGER NOT NULL,
    amount         INTEGER NOT NULL,
    note           TEXT,
    parent_line_id INTEGER REFERENCES sale_lines (id)  -- a paid add-on is its own line
);

CREATE INDEX idx_sale_lines_sale ON sale_lines (sale_id);

CREATE TABLE payments (
    id       INTEGER PRIMARY KEY,
    sale_id  INTEGER NOT NULL REFERENCES sales (id),
    method   TEXT    NOT NULL,  -- Cash | QRIS, from settings.payment_methods
    amount   INTEGER NOT NULL,
    received INTEGER,           -- cash only
    change   INTEGER            -- cash only
);

CREATE INDEX idx_payments_sale ON payments (sale_id);

CREATE TABLE shift_counts (
    id       INTEGER PRIMARY KEY,
    shift_id INTEGER NOT NULL REFERENCES shifts (id),
    method   TEXT    NOT NULL,
    counted  INTEGER NOT NULL,  -- typed by the cashier; never auto-filled from expected
    expected INTEGER NOT NULL,
    diff     INTEGER NOT NULL
);

CREATE TABLE stock_moves (
    id      INTEGER PRIMARY KEY,
    item_id INTEGER NOT NULL REFERENCES items (id),
    qty     REAL    NOT NULL,  -- + in / - out; the balance is SUM(qty), never a column
    reason  TEXT    NOT NULL CHECK (
        reason IN ('opening', 'sale', 'void', 'opname', 'purchase', 'waste', 'return')
    ),
    ref     TEXT,
    cost    INTEGER NOT NULL DEFAULT 0,
    at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);

CREATE INDEX idx_stock_moves_item ON stock_moves (item_id);
CREATE INDEX idx_stock_moves_at ON stock_moves (at);

CREATE TABLE vouchers (
    id               INTEGER PRIMARY KEY,
    sale_id          INTEGER NOT NULL REFERENCES sales (id),
    item_id          INTEGER NOT NULL REFERENCES items (id),
    code             TEXT    NOT NULL,
    password         TEXT    NOT NULL,
    profile          TEXT,
    duration_minutes INTEGER,
    validity_minutes INTEGER,
    status           TEXT    NOT NULL DEFAULT 'provisioned'
        CHECK (status IN ('provisioned', 'failed', 'printed')),
    router_ref       TEXT,
    created_at       TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
    printed_at       TEXT
);

CREATE INDEX idx_vouchers_sale ON vouchers (sale_id);

-- Monthly aggregates imported from ERPNext so old revenue stays reportable without
-- carrying the old invoice schema around.
CREATE TABLE legacy_totals (
    period   TEXT    PRIMARY KEY,  -- YYYY-MM
    invoices INTEGER NOT NULL,
    revenue  INTEGER NOT NULL,
    note     TEXT
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
