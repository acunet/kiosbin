//! Bootstrap import of the live ERPNext catalogue snapshot (`seed/warkop_seed.json`).
//!
//! The seed is an *initial* import: by default existing rows are left alone, so a price
//! the owner changed inside the POS is never silently reverted by re-running it.
//! `--force` re-reads the catalogue fields (name / group / price / uom / kind / active)
//! for a deliberate "menu changed in the source, refresh it" pass.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, BoxError>;

#[derive(Debug, Deserialize)]
pub struct SeedFile {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub items: Vec<SeedItem>,
    #[serde(default)]
    pub raw_materials: Vec<SeedRaw>,
    #[serde(default)]
    pub payment_methods: Vec<SeedPaymentMethod>,
    #[serde(default)]
    pub printing: Option<SeedPrinting>,
}

#[derive(Debug, Deserialize)]
pub struct SeedItem {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub price: i64,
    #[serde(default)]
    pub uom: String,
    #[serde(default = "one")]
    pub active: i64,
    #[serde(default)]
    pub recipe: Vec<SeedRecipeLine>,
}

#[derive(Debug, Deserialize)]
pub struct SeedRaw {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub uom: String,
}

#[derive(Debug, Deserialize)]
pub struct SeedRecipeLine {
    pub item: String,
    pub qty: f64,
    #[serde(default)]
    pub uom: String,
}

#[derive(Debug, Deserialize)]
pub struct SeedPaymentMethod {
    pub name: String,
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Deserialize)]
pub struct SeedPrinting {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<i64>,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Default)]
pub struct SeedReport {
    pub items_written: u64,
    pub items_kept: u64,
    pub recipes_written: u64,
    pub uom_conversions_added: u64,
    pub settings_added: u64,
}

/// Item taxonomy: raw materials by prefix, services by group, paid extras as add-ons.
pub fn kind_for(code: &str, group: &str) -> &'static str {
    if code.starts_with("BB-") {
        "raw"
    } else if group == "Services" {
        "service"
    } else if group == "Add-ons" || code.starts_with("MN-AD-") {
        "addon"
    } else {
        "menu"
    }
}

pub async fn seed(pool: &SqlitePool, path: &Path, force: bool) -> Result<SeedReport> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read seed file {}: {e}", path.display()))?;
    let file: SeedFile = serde_json::from_str(&text)
        .map_err(|e| format!("seed file {} is not valid JSON: {e}", path.display()))?;

    let mut tx = pool.begin().await?;
    let mut report = SeedReport::default();

    // Raw materials first, so a recipe can never point at a row that does not exist yet.
    for row in &file.raw_materials {
        let uom = if row.uom.is_empty() { "Pcs" } else { &row.uom };
        upsert_item(
            &mut tx,
            &row.code,
            &row.name,
            "",
            0,
            uom,
            "raw",
            1,
            force,
            &mut report,
        )
        .await?;
    }
    for item in &file.items {
        let uom = if item.uom.is_empty() {
            "Portion"
        } else {
            &item.uom
        };
        let kind = kind_for(&item.code, &item.group);
        upsert_item(
            &mut tx,
            &item.code,
            &item.name,
            &item.group,
            item.price,
            uom,
            kind,
            item.active,
            force,
            &mut report,
        )
        .await?;
    }

    let ids = item_ids(&mut tx).await?;

    for item in &file.items {
        let parent = *ids
            .get(&item.code)
            .ok_or_else(|| format!("item {} is missing after insert", item.code))?;
        for line in &item.recipe {
            let ingredient = *ids.get(&line.item).ok_or_else(|| {
                format!(
                    "recipe for {} references unknown ingredient {} — fix the seed, do not skip it",
                    item.code, line.item
                )
            })?;
            if !line.qty.is_finite() || line.qty <= 0.0 {
                return Err(format!(
                    "recipe line {} -> {} has qty={} (must be > 0)",
                    item.code, line.item, line.qty
                )
                .into());
            }
            let uom = if line.uom.is_empty() {
                "Gram"
            } else {
                &line.uom
            };
            let sql = if force {
                "INSERT INTO recipes (item_id, ingredient_id, qty, uom) VALUES (?, ?, ?, ?)
                 ON CONFLICT (item_id, ingredient_id) DO UPDATE SET qty = excluded.qty, uom = excluded.uom"
            } else {
                "INSERT INTO recipes (item_id, ingredient_id, qty, uom) VALUES (?, ?, ?, ?)
                 ON CONFLICT (item_id, ingredient_id) DO NOTHING"
            };
            let done = sqlx::query(sql)
                .bind(parent)
                .bind(ingredient)
                .bind(line.qty)
                .bind(uom)
                .execute(&mut *tx)
                .await?;
            report.recipes_written += done.rows_affected();
        }
    }

    // Purchasing UOMs needed once purchase entry (HPP) lands. Consumption UOMs are the
    // ledger base and need no row: from_uom = to_uom is factor 1 by definition.
    for (from, to, factor) in [
        ("Kg", "Gram", 1000.0),
        ("Liter", "Ml", 1000.0),
        ("Box", "Pcs", 24.0),
        ("Carton", "Pack", 12.0),
    ] {
        let done = sqlx::query(
            "INSERT INTO uom_conversions (from_uom, to_uom, factor) VALUES (?, ?, ?)
             ON CONFLICT (from_uom, to_uom) DO NOTHING",
        )
        .bind(from)
        .bind(to)
        .bind(factor)
        .execute(&mut *tx)
        .await?;
        report.uom_conversions_added += done.rows_affected();
    }

    let methods: Vec<&str> = file
        .payment_methods
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    let printing_host = file
        .printing
        .as_ref()
        .and_then(|p| p.host.clone())
        .unwrap_or_default();
    let printing_port = file.printing.as_ref().and_then(|p| p.port).unwrap_or(9100);

    let defaults = [
        ("outlet_name", "Warkop 3Dara".to_string()),
        // The method whose money ends up in the drawer; kept as data, not baked into code.
        ("cash_method", "Cash".to_string()),
        ("qris_method", "QRIS".to_string()),
        ("payment_methods", methods.join(",")),
        ("printer_host", printing_host),
        ("printer_port", printing_port.to_string()),
        // 48 columns = the 80 mm layout the old POS printed. 58 mm would be 32.
        ("paper_cols", "48".to_string()),
        ("qris_image", String::new()),
    ];
    for (key, value) in defaults {
        let done = sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO NOTHING",
        )
        .bind(key)
        .bind(value)
        .execute(&mut *tx)
        .await?;
        report.settings_added += done.rows_affected();
    }

    // Provenance is always refreshed — that is what tells you which snapshot is loaded.
    for (key, value) in [
        ("seed_source", file.source.clone().unwrap_or_default()),
        ("seeded_at", String::new()),
    ] {
        let sql = if key == "seeded_at" {
            "INSERT INTO settings (key, value) VALUES (?, datetime('now', 'localtime'))
             ON CONFLICT (key) DO UPDATE SET value = datetime('now', 'localtime')"
        } else {
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value"
        };
        sqlx::query(sql)
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
async fn upsert_item(
    tx: &mut Transaction<'_, Sqlite>,
    code: &str,
    name: &str,
    group: &str,
    price: i64,
    uom: &str,
    kind: &str,
    active: i64,
    force: bool,
    report: &mut SeedReport,
) -> Result<()> {
    let sql = if force {
        "INSERT INTO items (code, name, group_name, price, uom, kind, active) VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (code) DO UPDATE SET name = excluded.name, group_name = excluded.group_name,
             price = excluded.price, uom = excluded.uom, kind = excluded.kind, active = excluded.active"
    } else {
        "INSERT INTO items (code, name, group_name, price, uom, kind, active) VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (code) DO NOTHING"
    };
    let done = sqlx::query(sql)
        .bind(code)
        .bind(name)
        .bind(group)
        .bind(price)
        .bind(uom)
        .bind(kind)
        .bind(active)
        .execute(&mut **tx)
        .await?;

    if done.rows_affected() > 0 {
        report.items_written += 1;
    } else {
        report.items_kept += 1;
    }
    Ok(())
}

async fn item_ids(tx: &mut Transaction<'_, Sqlite>) -> Result<HashMap<String, i64>> {
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, code FROM items")
        .fetch_all(&mut **tx)
        .await?;
    Ok(rows.into_iter().map(|(id, code)| (code, id)).collect())
}
