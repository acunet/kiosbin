//! Shifts and cash reconciliation.
//!
//! Two rules from the ERP install are encoded here: `counted` is always typed by the
//! cashier (never auto-filled from `expected`), and expected cash excludes every
//! non-cash method.

use sqlx::SqlitePool;

use crate::settings::{BoxError, Result, Settings};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Shift {
    pub id: i64,
    pub cashier: String,
    pub opened_at: String,
    pub opening_float: i64,
}

#[derive(Debug, Clone)]
pub struct MethodExpected {
    pub method: String,
    pub expected: i64,
    pub is_cash: bool,
}

#[derive(Debug, Clone)]
pub struct CountLine {
    pub method: String,
    pub expected: i64,
    pub counted: i64,
    pub diff: i64,
    pub is_cash: bool,
}

pub async fn open(pool: &SqlitePool, cashier: &str, opening_float: i64) -> Result<i64> {
    let now: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(pool)
        .await?;
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO shifts (cashier, opened_at, opening_float, status) VALUES (?, ?, ?, 'open')
         RETURNING id",
    )
    .bind(cashier)
    .bind(now)
    .bind(opening_float)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        BoxError::from(format!(
            "cannot open a shift for {cashier}: {e} (a shift may already be open)"
        ))
    })?;
    Ok(id.0)
}

pub async fn open_for(pool: &SqlitePool, cashier: &str) -> Result<Option<Shift>> {
    let row = sqlx::query_as::<_, Shift>(
        "SELECT id, cashier, opened_at, opening_float FROM shifts
         WHERE cashier = ? AND status = 'open' ORDER BY id DESC LIMIT 1",
    )
    .bind(cashier)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn by_id(pool: &SqlitePool, id: i64) -> Result<Option<Shift>> {
    let row = sqlx::query_as::<_, Shift>(
        "SELECT id, cashier, opened_at, opening_float FROM shifts WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Expected money per payment method for one shift, recomputed from the ledger every time.
pub async fn expected(
    pool: &SqlitePool,
    shift: &Shift,
    settings: &Settings,
) -> Result<Vec<MethodExpected>> {
    let mut methods = settings.payment_methods();
    if methods.is_empty() {
        methods.push(settings.cash_method());
    }
    let cash_method = settings.cash_method();

    let mut out = Vec::with_capacity(methods.len());
    for method in methods {
        let (taken,): (Option<i64>,) = sqlx::query_as(
            "SELECT SUM(p.amount) FROM payments p
             JOIN sales s ON s.id = p.sale_id
             WHERE s.shift_id = ? AND s.status = 'paid' AND p.method = ?",
        )
        .bind(shift.id)
        .bind(&method)
        .fetch_one(pool)
        .await?;

        let is_cash = method == cash_method;
        let taken = taken.unwrap_or(0);
        out.push(MethodExpected {
            method,
            expected: if is_cash {
                shift.opening_float + taken
            } else {
                taken
            },
            is_cash,
        });
    }
    Ok(out)
}

/// Closes the shift. `counts` must carry a counted amount for every method — an empty
/// input is an error, not a zero, because that is how cash control stays alive.
pub async fn close(
    pool: &SqlitePool,
    shift: &Shift,
    settings: &Settings,
    counts: &[(String, Option<i64>)],
) -> Result<Vec<CountLine>> {
    let expected = expected(pool, shift, settings).await?;

    let mut lines = Vec::with_capacity(expected.len());
    for row in expected {
        let counted = counts
            .iter()
            .find(|(method, _)| method == &row.method)
            .and_then(|(_, counted)| *counted)
            .ok_or_else(|| {
                BoxError::from(format!("{} {}", settings.t().count_required, row.method))
            })?;
        lines.push(CountLine {
            method: row.method,
            expected: row.expected,
            counted,
            diff: counted - row.expected,
            is_cash: row.is_cash,
        });
    }

    let now: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(pool)
        .await?;
    let mut tx = pool.begin().await?;
    for line in &lines {
        sqlx::query(
            "INSERT INTO shift_counts (shift_id, method, counted, expected, diff)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(shift.id)
        .bind(&line.method)
        .bind(line.counted)
        .bind(line.expected)
        .bind(line.diff)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE shifts SET status = 'closed', closed_at = ? WHERE id = ?")
        .bind(&now)
        .bind(shift.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(lines)
}

/// What this shift has taken so far, per method — shown on the till header.
pub async fn taken_per_method(pool: &SqlitePool, shift_id: i64) -> Result<Vec<(String, i64)>> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT p.method, SUM(p.amount) FROM payments p
         JOIN sales s ON s.id = p.sale_id
         WHERE s.shift_id = ? AND s.status = 'paid'
         GROUP BY p.method ORDER BY p.method",
    )
    .bind(shift_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
