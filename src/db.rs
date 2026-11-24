//! Database open + migration. One SQLite file is the whole datastore.

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

/// Opens (creating it when missing) the POS database with WAL + foreign keys enforced.
pub async fn open(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
}

/// Runs every pending migration in `dir`. Safe to call on an empty file and on a
/// file that is already up to date (sqlx keeps `_sqlx_migrations`).
pub async fn migrate(pool: &SqlitePool, dir: &Path) -> Result<(), sqlx::migrate::MigrateError> {
    let migrator = sqlx::migrate::Migrator::new(dir).await?;
    migrator.run(pool).await
}
