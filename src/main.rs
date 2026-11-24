//! RustPOS command line entry point.
//!
//!   rustpos migrate [--db PATH] [--migrations DIR]
//!   rustpos import <fixture.json> [--force] [--db PATH]
//!   rustpos check [--db PATH]
//!
//! The web server (`rustpos serve`) lands in the next step: it is served by this same
//! binary, which is the point — one artifact to install on the shop's PC.

use std::path::PathBuf;
use std::process::ExitCode;

use rustpos::{db, import};

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn db_path(args: &[String]) -> PathBuf {
    flag(args, "--db")
        .or_else(|| std::env::var("RUSTPOS_DB").ok())
        .unwrap_or_else(|| "rustpos.db".to_string())
        .into()
}

fn usage() {
    eprintln!(
        "rustpos <command>\n\
         \n\
         commands:\n\
         \x20 migrate [--db PATH] [--migrations DIR]    create/upgrade the SQLite database\n\
         \x20 import <fixture.json> [--force] [--db PATH] load an outlet catalog fixture\n\
         \x20 check [--db PATH]                          print row counts and stock sanity"
    );
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        usage();
        return ExitCode::FAILURE;
    };
    let path = db_path(&args);

    let result = match command.as_str() {
        "migrate" => {
            let dir: PathBuf = flag(&args, "--migrations")
                .unwrap_or_else(|| "migrations".to_string())
                .into();
            match db::open(&path).await {
                Ok(pool) => match db::migrate(&pool, &dir).await {
                    Ok(()) => {
                        println!(
                            "migrate: ok  db={}  migrations={}",
                            path.display(),
                            dir.display()
                        );
                        Ok(())
                    }
                    Err(e) => Err(format!("migrate failed: {e}")),
                },
                Err(e) => Err(format!("cannot open {}: {e}", path.display())),
            }
        }
        "import" => match args.get(1) {
            None => Err("import needs a fixture file, e.g. `rustpos import fixtures/shop.seed.json`".into()),
            Some(file) => match db::open(&path).await {
                Ok(pool) => match import::import_catalog(&pool, &PathBuf::from(file), has(&args, "--force")).await {
                    Ok(r) => {
                        println!(
                            "import: ok  items written={} kept={}  recipe lines={}  uom conversions added={}  settings added={}",
                            r.items_written, r.items_kept, r.recipes_written, r.uom_conversions_added, r.settings_added
                        );
                        Ok(())
                    }
                    Err(e) => Err(format!("import failed: {e}")),
                },
                Err(e) => Err(format!("cannot open {}: {e}", path.display())),
            },
        },
        "check" => match db::open(&path).await {
            Ok(pool) => match check(&pool).await {
                Ok(()) => Ok(()),
                Err(e) => Err(format!("check failed: {e}")),
            },
            Err(e) => Err(format!("cannot open {}: {e}", path.display())),
        },
        other => Err(format!("unknown command `{other}`")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            usage();
            ExitCode::FAILURE
        }
    }
}

async fn setting(pool: &sqlx::SqlitePool, key: &str) -> Result<String, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(value,)| value).unwrap_or_default())
}

async fn check(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    let kinds: Vec<(String, i64)> =
        sqlx::query_as("SELECT kind, COUNT(*) FROM items GROUP BY kind ORDER BY kind")
            .fetch_all(pool)
            .await?;
    let recipes: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recipes").fetch_one(pool).await?;
    let moves: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM stock_moves").fetch_one(pool).await?;
    let sales: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sales").fetch_one(pool).await?;
    let settings: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM settings").fetch_one(pool).await?;
    let empty_price: Vec<(String, String)> = sqlx::query_as(
        "SELECT code, name FROM items WHERE price = 0 AND kind IN ('menu', 'addon') ORDER BY code",
    )
    .fetch_all(pool)
    .await?;
    let negative: Vec<(String, f64)> = sqlx::query_as(
        "SELECT i.code, SUM(m.qty) AS balance FROM items i JOIN stock_moves m ON m.item_id = i.id
         GROUP BY i.id HAVING balance <= 0 ORDER BY balance",
    )
    .fetch_all(pool)
    .await?;

    println!(
        "outlet = {}   currency = {} {}   invoice prefix = {}   paper = {} cols",
        setting(pool, "outlet_name").await?,
        setting(pool, "currency").await?,
        setting(pool, "currency_symbol").await?,
        setting(pool, "invoice_prefix").await?,
        setting(pool, "paper_cols").await?,
    );
    for (kind, count) in kinds {
        println!("items[{kind}] = {count}");
    }
    println!("recipes = {}", recipes.0);
    println!("stock_moves = {}", moves.0);
    println!("sales = {}", sales.0);
    println!("settings = {}", settings.0);
    println!("items with no price (till must ask) = {}", empty_price.len());
    for (code, name) in &empty_price {
        println!("  - {code} {name}");
    }
    println!("ingredients at or below zero = {}", negative.len());
    for (code, balance) in &negative {
        println!("  - {code} balance={balance}");
    }
    Ok(())
}
