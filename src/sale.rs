//! Writing a sale: numbers, money, payment and the stock deduction in **one** transaction.

use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::settings::{BoxError, Result, Settings};

#[derive(Debug)]
pub struct SaleInput {
    pub cashier: String,
    pub shift_id: i64,
    pub method: String,
    /// Cash only: what the customer handed over.
    pub received: Option<i64>,
    pub discount: i64,
    /// (item_id, qty) — prices are read from the database, never from the client.
    pub lines: Vec<(i64, i64)>,
}

#[derive(Debug, Clone)]
pub struct SaleLineView {
    pub item_id: i64,
    pub name: String,
    pub qty: i64,
    pub unit_price: i64,
    pub amount: i64,
}

#[derive(Debug, Clone)]
pub struct SaleResult {
    pub id: i64,
    pub number: String,
    pub at: String,
    pub cashier: String,
    pub lines: Vec<SaleLineView>,
    pub subtotal: i64,
    pub discount: i64,
    pub total: i64,
    pub method: String,
    pub received: Option<i64>,
    pub change: Option<i64>,
    pub stock_rows: u32,
}

/// `PREFIX-YYYYMMDD-NNN`, sequential per day, built inside the sale transaction.
async fn next_number(tx: &mut Transaction<'_, Sqlite>, prefix: &str) -> Result<String> {
    let now: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(&mut **tx)
        .await?;
    let day = now[..10].replace('-', "");
    let like = format!("{prefix}-{day}-%");

    let last: Option<(String,)> =
        sqlx::query_as("SELECT number FROM sales WHERE number LIKE ? ORDER BY number DESC LIMIT 1")
            .bind(&like)
            .fetch_optional(&mut **tx)
            .await?;

    let next = match last {
        Some((number,)) => {
            number
                .rsplit('-')
                .next()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0)
                + 1
        }
        None => 1,
    };
    Ok(format!("{prefix}-{day}-{next:03}"))
}

pub async fn create(
    pool: &SqlitePool,
    settings: &Settings,
    input: SaleInput,
) -> Result<SaleResult> {
    if input.lines.is_empty() {
        return Err("cart is empty".into());
    }

    let mut tx = pool.begin().await?;

    // 1. Re-read every price from the database. The client only ever sends item ids.
    let mut lines = Vec::with_capacity(input.lines.len());
    let mut subtotal = 0i64;
    for (item_id, qty) in &input.lines {
        if *qty <= 0 {
            return Err("a sale line has quantity 0".into());
        }
        let row: Option<(String, String, i64, i64)> =
            sqlx::query_as("SELECT code, name, price, active FROM items WHERE id = ?")
                .bind(item_id)
                .fetch_optional(&mut *tx)
                .await?;
        let (code, name, price, active) =
            row.ok_or_else(|| BoxError::from(format!("item {item_id} is not in the catalog")))?;
        if active == 0 {
            return Err(format!("{name} is not active any more").into());
        }
        // Never sell at zero: the till must ask for a price instead (see AGENTS.md §6).
        if price <= 0 {
            return Err(format!(
                "{code} {name} has no price yet — set its price before selling it"
            )
            .into());
        }
        let amount = price * qty;
        subtotal += amount;
        lines.push(SaleLineView {
            item_id: *item_id,
            name,
            qty: *qty,
            unit_price: price,
            amount,
        });
    }

    let discount = input.discount.max(0);
    if discount > subtotal {
        return Err("discount is larger than the subtotal".into());
    }
    let total = subtotal - discount;

    let number = next_number(&mut tx, &settings.invoice_prefix()).await?;
    let at: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(&mut *tx)
        .await?;

    let (sale_id,): (i64,) = sqlx::query_as(
        "INSERT INTO sales (number, shift_id, cashier, at, subtotal, discount, total, status)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'paid') RETURNING id",
    )
    .bind(&number)
    .bind(input.shift_id)
    .bind(&input.cashier)
    .bind(&at)
    .bind(subtotal)
    .bind(discount)
    .bind(total)
    .fetch_one(&mut *tx)
    .await?;

    for line in &lines {
        sqlx::query(
            "INSERT INTO sale_lines (sale_id, item_id, name, qty, unit_price, amount)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(sale_id)
        .bind(line.item_id)
        .bind(&line.name)
        .bind(line.qty)
        .bind(line.unit_price)
        .bind(line.amount)
        .execute(&mut *tx)
        .await?;
    }

    // 2. Payment. Cash keeps what was handed over so the change can be reprinted.
    let is_cash = input.method == settings.cash_method();
    let (received, change) = if is_cash {
        let received = input
            .received
            .ok_or_else(|| BoxError::from("cash payment needs the amount handed over"))?;
        if received < total {
            return Err("cash received is less than the total".into());
        }
        (Some(received), Some(received - total))
    } else {
        (None, None)
    };

    sqlx::query(
        "INSERT INTO payments (sale_id, method, amount, received, change) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(sale_id)
    .bind(&input.method)
    .bind(total)
    .bind(received)
    .bind(change)
    .execute(&mut *tx)
    .await?;

    // 3. Ingredients leave the shelf in the same transaction as the money arriving.
    let stock_rows = crate::stock::deduct_for_sale(&mut tx, &number, &input.lines).await?;
    sqlx::query("UPDATE sales SET deducted_at = ? WHERE id = ?")
        .bind(&at)
        .bind(sale_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(SaleResult {
        id: sale_id,
        number,
        at,
        cashier: input.cashier,
        lines,
        subtotal,
        discount,
        total,
        method: input.method,
        received,
        change,
        stock_rows,
    })
}

pub async fn by_id(pool: &SqlitePool, sale_id: i64) -> Result<Option<SaleResult>> {
    let head: Option<(i64, String, String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT id, number, cashier, at, subtotal, discount, total FROM sales WHERE id = ?",
    )
    .bind(sale_id)
    .fetch_optional(pool)
    .await?;
    let Some((id, number, cashier, at, subtotal, discount, total)) = head else {
        return Ok(None);
    };

    let line_rows: Vec<(i64, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT item_id, name, qty, unit_price, amount FROM sale_lines WHERE sale_id = ? ORDER BY id",
    )
    .bind(sale_id)
    .fetch_all(pool)
    .await?;

    let payment: Option<(String, i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT method, amount, received, change FROM payments WHERE sale_id = ? LIMIT 1",
    )
    .bind(sale_id)
    .fetch_optional(pool)
    .await?;
    let (method, _amount, received, change) = payment.unwrap_or_default();

    Ok(Some(SaleResult {
        id,
        number,
        at,
        cashier,
        lines: line_rows
            .into_iter()
            .map(|(item_id, name, qty, unit_price, amount)| SaleLineView {
                item_id,
                name,
                qty,
                unit_price,
                amount,
            })
            .collect(),
        subtotal,
        discount,
        total,
        method,
        received,
        change,
        stock_rows: 0,
    }))
}

/// Today's numbers for the till header: how much came in, and in how many receipts.
pub async fn today_totals(pool: &SqlitePool, prefix: &str) -> Result<(i64, i64)> {
    let now: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(pool)
        .await?;
    let day = now[..10].replace('-', "");
    let like = format!("{prefix}-{day}-%");
    let (count, sum): (i64, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), SUM(total) FROM sales WHERE status = 'paid' AND number LIKE ?",
    )
    .bind(like)
    .fetch_one(pool)
    .await?;
    Ok((count, sum.unwrap_or(0)))
}

/// The number the next sale *will* get — shown to the cashier, never reserved here.
pub async fn next_number_hint(pool: &SqlitePool, prefix: &str) -> Result<String> {
    let now: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(pool)
        .await?;
    let day = now[..10].replace('-', "");
    let like = format!("{prefix}-{day}-%");
    let last: Option<(String,)> =
        sqlx::query_as("SELECT number FROM sales WHERE number LIKE ? ORDER BY number DESC LIMIT 1")
            .bind(&like)
            .fetch_optional(pool)
            .await?;
    let next = match last {
        Some((number,)) => {
            number
                .rsplit('-')
                .next()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0)
                + 1
        }
        None => 1,
    };
    Ok(format!("{prefix}-{day}-{next:03}"))
}

/// Voiding is a reversal: status changes, stock comes back, the number is never reused.
pub async fn void(pool: &SqlitePool, sale_id: i64, reason: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    let row: Option<(String,)> = sqlx::query_as("SELECT number FROM sales WHERE id = ?")
        .bind(sale_id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some((number,)) = row else {
        return Err("sale not found".into());
    };

    let moves: Vec<(i64, f64)> =
        sqlx::query_as("SELECT item_id, qty FROM stock_moves WHERE ref = ? AND reason = 'sale'")
            .bind(&number)
            .fetch_all(&mut *tx)
            .await?;

    let now: String = sqlx::query_scalar(crate::db::NOW_SQL)
        .fetch_one(&mut *tx)
        .await?;
    for (item_id, qty) in moves {
        sqlx::query(
            "INSERT INTO stock_moves (item_id, qty, reason, ref, at)
             VALUES (?, ?, 'void', ?, ?)",
        )
        .bind(item_id)
        .bind(-qty)
        .bind(format!("void {number}: {reason}"))
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query("UPDATE sales SET status = 'void' WHERE id = ?")
        .bind(sale_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
