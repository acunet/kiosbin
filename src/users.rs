//! Cashiers and their PIN.
//!
//! The PIN is stored as a salted SHA-256 hash, so reading the database file does not hand
//! over the till. A 4–6 digit PIN is not strong cryptography — it stops someone reading the
//! database, not someone guessing in front of the tablet — so failed attempts are throttled
//! by the login handler.

use sqlx::SqlitePool;

use crate::settings::{BoxError, Result};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Cashier {
    pub id: i64,
    pub name: String,
    pub role: String,
}

fn hash_pin(pin: &str, salt: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b":");
    hasher.update(pin.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub async fn create(pool: &SqlitePool, name: &str, pin: &str, role: &str) -> Result<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err("cashier name cannot be empty".into());
    }
    if pin.len() < 4 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("PIN must be 4-6 digits".into());
    }
    if role != "cashier" && role != "owner" {
        return Err("role must be cashier or owner".into());
    }

    // Randomness comes from SQLite, so no dependency is needed just to make a salt.
    let (salt,): (String,) = sqlx::query_as("SELECT lower(hex(randomblob(16)))")
        .fetch_one(pool)
        .await?;
    let hash = hash_pin(pin, &salt);

    let id: (i64,) = sqlx::query_as(
        "INSERT INTO users (name, pin_salt, pin_hash, role) VALUES (?, ?, ?, ?)
         ON CONFLICT (name) DO UPDATE SET pin_salt = excluded.pin_salt,
             pin_hash = excluded.pin_hash, role = excluded.role
         RETURNING id",
    )
    .bind(name)
    .bind(&salt)
    .bind(&hash)
    .bind(role)
    .fetch_one(pool)
    .await
    .map_err(|e| BoxError::from(format!("cannot save cashier {name}: {e}")))?;
    Ok(id.0)
}

pub async fn verify(pool: &SqlitePool, name: &str, pin: &str) -> Result<Option<Cashier>> {
    let row: Option<(i64, String, String, String, String)> = sqlx::query_as(
        "SELECT id, name, role, pin_salt, pin_hash FROM users WHERE name = ? AND active = 1",
    )
    .bind(name.trim())
    .fetch_optional(pool)
    .await?;

    let Some((id, name, role, salt, expected)) = row else {
        return Ok(None);
    };
    if hash_pin(pin, &salt) != expected {
        return Ok(None);
    }
    Ok(Some(Cashier { id, name, role }))
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<Cashier>> {
    let rows = sqlx::query_as::<_, Cashier>(
        "SELECT id, name, role FROM users WHERE active = 1 ORDER BY role DESC, name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_pin_with_a_different_salt_hashes_differently() {
        assert_ne!(hash_pin("1234", "aaaa"), hash_pin("1234", "bbbb"));
        assert_eq!(hash_pin("1234", "aaaa"), hash_pin("1234", "aaaa"));
    }
}
