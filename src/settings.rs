//! Outlet settings, loaded once from the `settings` table (a fixture writes them).

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::money::Currency;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, BoxError>;

#[derive(Debug, Clone, Default)]
pub struct Settings {
    map: HashMap<String, String>,
}

impl Settings {
    pub fn from_map(map: HashMap<String, String>) -> Self {
        Settings { map }
    }

    pub fn get(&self, key: &str) -> String {
        self.map.get(key).cloned().unwrap_or_default()
    }

    pub fn outlet_name(&self) -> String {
        self.get("outlet_name")
    }

    pub fn invoice_prefix(&self) -> String {
        let prefix = self.get("invoice_prefix");
        if prefix.is_empty() {
            "POS".to_string()
        } else {
            prefix
        }
    }

    /// The method whose money lands in the drawer. Data, never baked into code.
    pub fn cash_method(&self) -> String {
        self.get("cash_method")
    }

    pub fn payment_methods(&self) -> Vec<String> {
        self.get("payment_methods")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    pub fn paper_cols(&self) -> usize {
        self.get("paper_cols").parse().unwrap_or(48)
    }

    pub fn currency(&self) -> Currency {
        Currency {
            symbol: {
                let symbol = self.get("currency_symbol");
                if symbol.is_empty() {
                    "Rp".to_string()
                } else {
                    symbol
                }
            },
            decimals: self.get("currency_decimals").parse().unwrap_or(0),
            group_sep: '.',
        }
    }

    pub fn money(&self, amount: i64) -> String {
        self.currency().format(amount)
    }
}

pub async fn load(pool: &SqlitePool) -> Result<Settings> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await?;
    Ok(Settings {
        map: rows.into_iter().collect(),
    })
}
