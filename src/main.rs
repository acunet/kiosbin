//! Kiosbin command line.
//!
//!   kiosbin migrate [--db PATH] [--migrations DIR]
//!   kiosbin import <fixture.json> [--force] [--db PATH]
//!   kiosbin user add <name> [--role cashier|owner] [--db PATH]      (PIN on stdin)
//!   kiosbin serve [--bind ADDR] [--db PATH]
//!   kiosbin check [--db PATH]
//!
//! The web server lives in this same binary: one artifact to install on the shop's PC.

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;

use kiosbin::{db, import, users, web};

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
        .or_else(|| std::env::var("KIOSBIN_DB").ok())
        .unwrap_or_else(|| "kiosbin.db".to_string())
        .into()
}

fn usage() {
    eprintln!(
        "kiosbin <command>\n\
         \n\
         commands:\n\
         \x20 migrate [--db PATH] [--migrations DIR]        create/upgrade the SQLite database\n\
         \x20 import <fixture.json> [--force] [--db PATH]    load an outlet catalog fixture\n\
         \x20 user add <name> [--role cashier|owner]         create a cashier (PIN read from stdin)\n\
         \x20 serve [--bind ADDR] [--db PATH]                run the till UI (default 0.0.0.0:8080)\n\
         \x20 check [--db PATH]                              print row counts and stock sanity"
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

    let result: Result<(), String> = match command.as_str() {
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
            None => Err(
                "import needs a fixture file, e.g. `kiosbin import fixtures/shop.seed.json`".into(),
            ),
            Some(file) => match db::open(&path).await {
                Ok(pool) => {
                    match import::import_catalog(&pool, &PathBuf::from(file), has(&args, "--force"))
                        .await
                    {
                        Ok(r) => {
                            println!(
                            "import: ok  items written={} kept={}  recipe lines={}  uom conversions added={}  settings added={}",
                            r.items_written, r.items_kept, r.recipes_written, r.uom_conversions_added, r.settings_added
                        );
                            Ok(())
                        }
                        Err(e) => Err(format!("import failed: {e}")),
                    }
                }
                Err(e) => Err(format!("cannot open {}: {e}", path.display())),
            },
        },
        "user" => user_command(&args, &path).await,
        "serve" => {
            let bind = flag(&args, "--bind")
                .or_else(|| std::env::var("KIOSBIN_BIND").ok())
                .unwrap_or_else(|| "0.0.0.0:8080".to_string());
            match db::open(&path).await {
                Ok(pool) => {
                    match tokio::net::TcpListener::bind(&bind).await {
                        Ok(listener) => {
                            println!(
                                "serve: ok  db={}  listening on http://{bind}",
                                path.display()
                            );
                            println!("till: open that address from a tablet or browser on the same network");
                            match axum::serve(listener, web::router(pool)).await {
                                Ok(()) => Ok(()),
                                Err(e) => Err(format!("server stopped: {e}")),
                            }
                        }
                        Err(e) => Err(format!("cannot listen on {bind}: {e}")),
                    }
                }
                Err(e) => Err(format!("cannot open {}: {e}", path.display())),
            }
        }
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

async fn user_command(args: &[String], path: &std::path::Path) -> Result<(), String> {
    let usage = "usage: kiosbin user add <name> [--role cashier|owner]";
    if args.get(1).map(String::as_str) != Some("add") {
        return Err(usage.into());
    }
    let name = args.get(2).ok_or_else(|| usage.to_string())?;
    let role = flag(args, "--role").unwrap_or_else(|| "cashier".to_string());
    let pin = read_pin(name)?;

    let pool = db::open(path)
        .await
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let id = users::create(&pool, name, pin.trim(), &role)
        .await
        .map_err(|e| format!("cannot create the cashier: {e}"))?;
    println!("user: ok  {name} ({role}) id={id}");
    Ok(())
}

fn read_pin(name: &str) -> Result<String, String> {
    eprint!("PIN for {name} (4-6 digits): ");
    let mut pin = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut pin)
        .map_err(|e| format!("cannot read the PIN: {e}"))?;
    Ok(pin)
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
    let recipes: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recipes")
        .fetch_one(pool)
        .await?;
    let moves: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM stock_moves")
        .fetch_one(pool)
        .await?;
    let (sales, revenue): (i64, Option<i64>) =
        sqlx::query_as("SELECT COUNT(*), SUM(total) FROM sales WHERE status = 'paid'")
            .fetch_one(pool)
            .await?;
    let cashiers: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    let settings: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM settings")
        .fetch_one(pool)
        .await?;
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
    println!("sales = {}  revenue = {}", sales, revenue.unwrap_or(0));
    println!("cashiers = {}", cashiers.0);
    println!("settings = {}", settings.0);
    println!(
        "items with no price (till must ask) = {}",
        empty_price.len()
    );
    for (code, name) in &empty_price {
        println!("  - {code} {name}");
    }
    println!("ingredients at or below zero = {}", negative.len());
    for (code, balance) in &negative {
        println!("  - {code} balance={balance}");
    }
    Ok(())
}
