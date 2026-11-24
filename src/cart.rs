//! The till cart, held in the database.
//!
//! Every change is a single atomic statement, so two fast taps can never lose an item, and an
//! unfinished order survives the server restarting mid-shift. Only item ids and quantities are
//! stored: names and prices are read from the catalog when the cart is rendered or charged.

use sqlx::SqlitePool;

use crate::settings::Result;

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct CartLine {
    pub item_id: i64,
    pub qty: i64,
}

pub async fn lines(pool: &SqlitePool, session_id: &str) -> Result<Vec<CartLine>> {
    let rows = sqlx::query_as::<_, CartLine>(
        "SELECT item_id, qty FROM cart_lines WHERE session_id = ? ORDER BY item_id",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Bumps the quantity in one statement — this is what makes rapid tapping safe.
pub async fn add(pool: &SqlitePool, session_id: &str, item_id: i64, qty: i64) -> Result<()> {
    if qty <= 0 {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO cart_lines (session_id, item_id, qty) VALUES (?, ?, ?)
         ON CONFLICT (session_id, item_id) DO UPDATE SET qty = qty + excluded.qty",
    )
    .bind(session_id)
    .bind(item_id)
    .bind(qty)
    .execute(pool)
    .await?;
    Ok(())
}

/// Sets an absolute quantity; zero or less removes the line (that is the minus button).
pub async fn set_qty(pool: &SqlitePool, session_id: &str, item_id: i64, qty: i64) -> Result<()> {
    if qty <= 0 {
        return remove(pool, session_id, item_id).await;
    }
    sqlx::query("UPDATE cart_lines SET qty = ? WHERE session_id = ? AND item_id = ?")
        .bind(qty)
        .bind(session_id)
        .bind(item_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn remove(pool: &SqlitePool, session_id: &str, item_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM cart_lines WHERE session_id = ? AND item_id = ?")
        .bind(session_id)
        .bind(item_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear(pool: &SqlitePool, session_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM cart_lines WHERE session_id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Total quantity in the cart, for the header.
pub async fn total_qty(pool: &SqlitePool, session_id: &str) -> Result<i64> {
    let (total,): (Option<i64>,) =
        sqlx::query_as("SELECT SUM(qty) FROM cart_lines WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(pool)
            .await?;
    Ok(total.unwrap_or(0))
}
