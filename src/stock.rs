//! The stock ledger. A balance is always derived — never stored.

use std::collections::HashMap;

use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::settings::{BoxError, Result};

/// Ingredient balance = SUM of every move. One query, no cached column to go stale.
pub async fn balance(pool: &SqlitePool, item_id: i64) -> Result<f64> {
    let (total,): (Option<f64>,) =
        sqlx::query_as("SELECT SUM(qty) FROM stock_moves WHERE item_id = ?")
            .bind(item_id)
            .fetch_one(pool)
            .await?;
    Ok(total.unwrap_or(0.0))
}

/// Ingredients at or below zero, for the low-stock warning on the till.
pub async fn items_at_or_below_zero(pool: &SqlitePool) -> Result<HashMap<String, f64>> {
    let rows: Vec<(String, f64)> = sqlx::query_as(
        "SELECT i.code, SUM(m.qty) FROM items i JOIN stock_moves m ON m.item_id = i.id
         GROUP BY i.id HAVING SUM(m.qty) <= 0",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Converts a recipe quantity into the ingredient's base unit.
///
/// The ledger is always kept in the ingredient's own uom, so a recipe that measures in
/// grams while the item is stocked in kilos still deducts correctly. A missing conversion
/// is an error, never a silently wrong number.
pub async fn to_base_uom(
    pool: &SqlitePool,
    line_uom: &str,
    base_uom: &str,
    qty: f64,
) -> Result<f64> {
    if line_uom == base_uom || line_uom.is_empty() {
        return Ok(qty);
    }
    let factor: Option<(f64,)> =
        sqlx::query_as("SELECT factor FROM uom_conversions WHERE from_uom = ? AND to_uom = ?")
            .bind(line_uom)
            .bind(base_uom)
            .fetch_optional(pool)
            .await?;

    match factor {
        Some((factor,)) => Ok(qty * factor),
        None => Err(format!(
            "no UOM conversion from {line_uom} to {base_uom} — add it to the fixture instead of \
             guessing the quantity"
        )
        .into()),
    }
}

/// Writes the sale's ingredient moves inside the sale's own transaction.
///
/// Returns how many ledger rows were written. The caller stamps `sales.deducted_at` in the
/// same transaction, which is what makes a retry (double click, replay) a no-op.
pub async fn deduct_for_sale(
    tx: &mut Transaction<'_, Sqlite>,
    sale_number: &str,
    lines: &[(i64, i64)],
) -> Result<u32> {
    let mut written = 0;
    let now: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(&mut **tx)
        .await?;
    for (item_id, qty_sold) in lines {
        let recipe: Vec<(i64, f64, String)> =
            sqlx::query_as("SELECT ingredient_id, qty, uom FROM recipes WHERE item_id = ?")
                .bind(item_id)
                .fetch_all(&mut **tx)
                .await?;

        for (ingredient_id, recipe_qty, recipe_uom) in recipe {
            let (base_uom,): (String,) = sqlx::query_as("SELECT uom FROM items WHERE id = ?")
                .bind(ingredient_id)
                .fetch_one(&mut **tx)
                .await?;
            let total = convert_in_tx(
                &mut *tx,
                &recipe_uom,
                &base_uom,
                recipe_qty * *qty_sold as f64,
            )
            .await?;

            sqlx::query(
                "INSERT INTO stock_moves (item_id, qty, reason, ref, at)
                 VALUES (?, ?, 'sale', ?, ?)",
            )
            .bind(ingredient_id)
            .bind(-total)
            .bind(sale_number)
            .bind(&now)
            .execute(&mut **tx)
            .await?;
            written += 1;
        }
    }
    Ok(written)
}

async fn convert_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    line_uom: &str,
    base_uom: &str,
    qty: f64,
) -> Result<f64> {
    if line_uom == base_uom || line_uom.is_empty() {
        return Ok(qty);
    }
    let factor: Option<(f64,)> =
        sqlx::query_as("SELECT factor FROM uom_conversions WHERE from_uom = ? AND to_uom = ?")
            .bind(line_uom)
            .bind(base_uom)
            .fetch_optional(&mut **tx)
            .await?;
    match factor {
        Some((factor,)) => Ok(qty * factor),
        None => Err(BoxError::from(format!(
            "no UOM conversion from {line_uom} to {base_uom}"
        ))),
    }
}

/// Appends an opening/stock-take correction. Used by the stock-take step.
pub async fn record(
    pool: &SqlitePool,
    item_id: i64,
    qty: f64,
    reason: &str,
    reference: &str,
) -> Result<()> {
    let now: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(pool)
        .await?;
    sqlx::query("INSERT INTO stock_moves (item_id, qty, reason, ref, at) VALUES (?, ?, ?, ?, ?)")
        .bind(item_id)
        .bind(qty)
        .bind(reason)
        .bind(reference)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}
