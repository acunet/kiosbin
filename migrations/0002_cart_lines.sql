-- 0002_cart_lines.sql — the till cart lives in the database, not in the session cookie.
--
-- Why: a cart edit is a read-modify-write. With the cart in the session, the framework writes
-- the session record back only when the response is finished, so a cashier tapping three items
-- quickly lost one or two of them (the second request loaded the session state from before the
-- first one was persisted). In the database the increment is a single atomic statement, and an
-- unfinished order also survives a server restart.

CREATE TABLE cart_lines (
    session_id TEXT    NOT NULL,
    item_id    INTEGER NOT NULL REFERENCES items (id),
    qty        INTEGER NOT NULL CHECK (qty > 0),
    PRIMARY KEY (session_id, item_id)
);

CREATE INDEX idx_cart_lines_session ON cart_lines (session_id);
