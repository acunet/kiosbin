//! Catalog import from a tenant fixture.
//!
//! A fixture is a JSON file describing one outlet's starting point: outlet settings,
//! payment methods, printer target, UOM conversions, the item catalog and the recipes.
//! It is the only place tenant-specific naming lives — the binary itself stays generic.
//!
//! The import is *additive*: existing rows are kept, so a price the owner edited inside
//! the POS is never silently reverted by re-running it. `--force` re-reads the catalog
//! fields on purpose after the fixture itself changed.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::db;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, BoxError>;

const KINDS: [&str; 4] = ["menu", "addon", "raw", "service"];

#[derive(Debug, Deserialize)]
pub struct Fixture {
    #[serde(default)]
    pub tenant: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub outlet: Option<Outlet>,
    #[serde(default)]
    pub payment_methods: Vec<PaymentMethod>,
    #[serde(default)]
    pub printing: Option<Printing>,
    #[serde(default)]
    pub uom_conversions: Vec<UomConversion>,
    #[serde(default)]
    pub items: Vec<FixtureItem>,
    #[serde(default)]
    pub raw_materials: Vec<FixtureRaw>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Outlet {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub currency_decimals: Option<u32>,
    #[serde(default)]
    pub invoice_prefix: Option<String>,
    #[serde(default)]
    pub paper_cols: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PaymentMethod {
    pub name: String,
    #[serde(default)]
    pub kind: String, // cash | qris | card | other
}

#[derive(Debug, Deserialize)]
pub struct Printing {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UomConversion {
    pub from: String,
    pub to: String,
    pub factor: f64,
}

#[derive(Debug, Deserialize)]
pub struct FixtureItem {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub price: i64,
    #[serde(default)]
    pub uom: String,
    #[serde(default = "one")]
    pub active: i64,
    #[serde(default)]
    pub recipe: Vec<RecipeLine>,
}

#[derive(Debug, Deserialize)]
pub struct FixtureRaw {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub uom: String,
}

#[derive(Debug, Deserialize)]
pub struct RecipeLine {
    pub item: String,
    pub qty: f64,
    #[serde(default)]
    pub uom: String,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub items_written: u64,
    pub items_kept: u64,
    pub recipes_written: u64,
    pub uom_conversions_added: u64,
    pub settings_added: u64,
}

/// Fallback taxonomy for hand-written fixtures that omit `kind`.
/// Anything not matched here and not named by the fixture lands as a `menu` item.
pub fn infer_kind(code: &str, group: &str) -> &'static str {
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

pub async fn import_catalog(pool: &SqlitePool, path: &Path, force: bool) -> Result<ImportReport> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read fixture {}: {e}", path.display()))?;
    let fixture: Fixture = serde_json::from_str(&text)
        .map_err(|e| format!("fixture {} is not valid JSON: {e}", path.display()))?;

    let mut tx = pool.begin().await?;
    let mut report = ImportReport::default();

    for row in &fixture.raw_materials {
        let uom = if row.uom.is_empty() { "Pcs" } else { &row.uom };
        upsert_item(&mut tx, &row.code, &row.name, "", 0, uom, "raw", 1, force, &mut report).await?;
    }
    for item in &fixture.items {
        let uom = if item.uom.is_empty() { "Portion" } else { &item.uom };
        let kind = match item.kind.as_deref() {
            Some(kind) => kind,
            None => infer_kind(&item.code, &item.group),
        };
        if !KINDS.contains(&kind) {
            return Err(format!(
                "item {} has kind `{kind}` — expected one of {}",
                item.code,
                KINDS.join("/")
            )
            .into());
        }
        upsert_item(
            &mut tx, &item.code, &item.name, &item.group, item.price, uom, kind, item.active, force,
            &mut report,
        )
        .await?;
    }

    let ids = item_ids(&mut tx).await?;

    for item in &fixture.items {
        let parent = *ids
            .get(&item.code)
            .ok_or_else(|| format!("item {} is missing after insert", item.code))?;
        for line in &item.recipe {
            let ingredient = *ids.get(&line.item).ok_or_else(|| {
                format!(
                    "recipe for {} references unknown ingredient {} — fix the fixture, do not skip it",
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
            let uom = if line.uom.is_empty() { "Gram" } else { &line.uom };
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

    for conversion in &fixture.uom_conversions {
        let done = sqlx::query(
            "INSERT INTO uom_conversions (from_uom, to_uom, factor) VALUES (?, ?, ?)
             ON CONFLICT (from_uom, to_uom) DO NOTHING",
        )
        .bind(&conversion.from)
        .bind(&conversion.to)
        .bind(conversion.factor)
        .execute(&mut *tx)
        .await?;
        report.uom_conversions_added += done.rows_affected();
    }

    let outlet = OutletSettings::from_fixture(fixture.outlet);
    let methods: Vec<&str> = fixture
        .payment_methods
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    let cash_method = fixture
        .payment_methods
        .iter()
        .find(|m| m.kind == "cash")
        .map(|m| m.name.clone())
        .unwrap_or_default();
    let qris_method = fixture
        .payment_methods
        .iter()
        .find(|m| m.kind == "qris")
        .map(|m| m.name.clone())
        .unwrap_or_default();
    let now = sqlx::query_scalar::<_, String>(db::NOW_SQL).fetch_one(&mut *tx).await?;

    let defaults = [
        ("outlet_name", outlet.name),
        ("currency", outlet.currency),
        ("currency_symbol", outlet.symbol),
        ("currency_decimals", outlet.currency_decimals.to_string()),
        ("invoice_prefix", outlet.invoice_prefix),
        ("paper_cols", outlet.paper_cols.to_string()),
        ("payment_methods", methods.join(",")),
        // Which method's money lands in the drawer; data, never baked into code.
        ("cash_method", cash_method),
        ("qris_method", qris_method),
        ("qris_image", String::new()),
        ("printer_host", fixture.printing.as_ref().and_then(|p| p.host.clone()).unwrap_or_default()),
        ("printer_port", fixture.printing.as_ref().and_then(|p| p.port).unwrap_or(9100).to_string()),
        ("catalog_source", fixture.source.clone().unwrap_or_default()),
        ("catalog_loaded_at", now),
    ];
    for (key, value) in defaults {
        // Settings are the owner's: an existing value is never overwritten by an import.
        let done = sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO NOTHING")
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await?;
        report.settings_added += done.rows_affected();
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
    report: &mut ImportReport,
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

/// Product defaults, so a fixture only has to describe what differs from a plain outlet.
#[derive(Debug, Default)]
struct OutletSettings {
    name: String,
    currency: String,
    symbol: String,
    currency_decimals: u32,
    invoice_prefix: String,
    paper_cols: i64,
}

impl OutletSettings {
    fn from_fixture(outlet: Option<Outlet>) -> Self {
        let outlet = outlet.unwrap_or_default();
        OutletSettings {
            name: outlet.name.unwrap_or_else(|| "My Outlet".to_string()),
            currency: outlet.currency.unwrap_or_else(|| "IDR".to_string()),
            symbol: outlet.symbol.unwrap_or_else(|| "Rp".to_string()),
            currency_decimals: outlet.currency_decimals.unwrap_or(0),
            invoice_prefix: outlet.invoice_prefix.unwrap_or_else(|| "POS".to_string()),
            paper_cols: outlet.paper_cols.unwrap_or(48),
        }
    }
}
