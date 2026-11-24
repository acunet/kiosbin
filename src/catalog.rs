//! Item catalog queries used by the till.

use sqlx::SqlitePool;

use crate::settings::Result;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Item {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub group_name: String,
    pub price: i64,
    pub uom: String,
    pub kind: String,
}

/// Everything the till can sell, in one query: the grid is rendered from this.
pub async fn sellable(pool: &SqlitePool) -> Result<Vec<Item>> {
    let rows = sqlx::query_as::<_, Item>(
        "SELECT id, code, name, group_name, price, uom, kind FROM items
         WHERE kind IN ('menu', 'addon', 'service') AND active = 1
         ORDER BY group_name, name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn search(pool: &SqlitePool, query: &str) -> Result<Vec<Item>> {
    let needle = format!("%{}%", query.trim().to_lowercase());
    let rows = sqlx::query_as::<_, Item>(
        "SELECT id, code, name, group_name, price, uom, kind FROM items
         WHERE kind IN ('menu', 'addon', 'service') AND active = 1
           AND (lower(code) LIKE ? OR lower(name) LIKE ?)
         ORDER BY name LIMIT 40",
    )
    .bind(&needle)
    .bind(&needle)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn by_id(pool: &SqlitePool, id: i64) -> Result<Option<Item>> {
    let row = sqlx::query_as::<_, Item>(
        "SELECT id, code, name, group_name, price, uom, kind FROM items WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Ingredient ids with the qty and uom taken from the recipe of a sold item.
pub async fn recipe_of(pool: &SqlitePool, item_id: i64) -> Result<Vec<(i64, f64, String)>> {
    let rows: Vec<(i64, f64, String)> =
        sqlx::query_as("SELECT r.ingredient_id, r.qty, r.uom FROM recipes r WHERE r.item_id = ?")
            .bind(item_id)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}
