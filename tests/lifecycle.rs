//! End-to-end checks for what the money and stock paths depend on:
//! migrating from an empty file, the constraints the schema promises, and the seed.

use std::path::{Path, PathBuf};

use warkop_pos::{db, seed};

struct TempDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TempDb {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("warkop-pos-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("warkop.db");
        TempDb { dir, path }
    }

    async fn migrated(tag: &str) -> (Self, sqlx::SqlitePool) {
        let temp = TempDb::new(tag);
        let pool = db::open(&temp.path).await.expect("open");
        db::migrate(&pool, &migrations_dir())
            .await
            .expect("migrate");
        (temp, pool)
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

fn seed_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("seed/warkop_seed.json")
}

#[tokio::test]
async fn migrates_from_empty_and_is_repeatable() {
    let (_temp, pool) = TempDb::migrated("migrate").await;

    let tables: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'
         AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tables.0, 13, "schema tables created");

    // Running it again on an up-to-date database must not fail or duplicate anything.
    db::migrate(&pool, &migrations_dir())
        .await
        .expect("second migrate");
}

#[tokio::test]
async fn stock_move_for_unknown_item_is_rejected() {
    let (_temp, pool) = TempDb::migrated("fk").await;
    let result =
        sqlx::query("INSERT INTO stock_moves (item_id, qty, reason) VALUES (9999, 1, 'opening')")
            .execute(&pool)
            .await;
    assert!(
        result.is_err(),
        "foreign key on stock_moves.item_id must hold"
    );
}

#[tokio::test]
async fn cashier_cannot_have_two_open_shifts() {
    let (_temp, pool) = TempDb::migrated("shift").await;
    sqlx::query("INSERT INTO shifts (cashier, opening_float) VALUES ('Rina', 100000)")
        .execute(&pool)
        .await
        .expect("first open shift");
    let second = sqlx::query("INSERT INTO shifts (cashier, opening_float) VALUES ('Rina', 50000)")
        .execute(&pool)
        .await;
    assert!(
        second.is_err(),
        "partial unique index must block a second open shift"
    );

    // A different cashier is still allowed, and a closed shift never blocks.
    sqlx::query("INSERT INTO shifts (cashier, opening_float) VALUES ('Dewi', 50000)")
        .execute(&pool)
        .await
        .expect("other cashier");
    sqlx::query(
        "UPDATE shifts SET status = 'closed', closed_at = datetime('now') WHERE cashier = 'Rina'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO shifts (cashier, opening_float) VALUES ('Rina', 20000)")
        .execute(&pool)
        .await
        .expect("new shift after closing the previous one");
}

#[tokio::test]
async fn seed_imports_the_live_catalogue_and_is_idempotent() {
    let (_temp, pool) = TempDb::migrated("seed").await;

    let first = seed::seed(&pool, &seed_file(), false)
        .await
        .expect("first seed");
    assert_eq!(
        first.items_written, 83,
        "39 menu/add-on/service + 44 raw materials"
    );

    let (items,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM items")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (recipes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recipes")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (raw,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM items WHERE kind = 'raw'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (hotspot,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM items WHERE kind = 'service'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(items, 83);
    assert_eq!(
        recipes, 70,
        "35 recipes / 70 ingredient lines in the snapshot"
    );
    assert_eq!(raw, 44);
    assert_eq!(hotspot, 1, "MN-HOTSPOT-2JAM lands as a service item");

    let second = seed::seed(&pool, &seed_file(), false)
        .await
        .expect("second seed");
    assert_eq!(
        second.items_written, 0,
        "re-running must not insert anything new"
    );
    assert_eq!(second.items_kept, 83);

    let (recipes_after,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recipes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(recipes_after, 70, "no duplicate recipe lines");
}

#[tokio::test]
async fn seed_keeps_prices_edited_inside_the_pos() {
    let (_temp, pool) = TempDb::migrated("price").await;
    seed::seed(&pool, &seed_file(), false).await.expect("seed");

    sqlx::query("UPDATE items SET price = 21000 WHERE code = 'MN-KOPI-03'")
        .execute(&pool)
        .await
        .unwrap();

    seed::seed(&pool, &seed_file(), false)
        .await
        .expect("re-seed");
    let (price,): (i64,) = sqlx::query_as("SELECT price FROM items WHERE code = 'MN-KOPI-03'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        price, 21000,
        "plain seed must not revert a manual price change"
    );

    seed::seed(&pool, &seed_file(), true)
        .await
        .expect("forced seed");
    let (forced,): (i64,) = sqlx::query_as("SELECT price FROM items WHERE code = 'MN-KOPI-03'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(forced, 18000, "--force re-reads the catalogue price");
}

#[tokio::test]
async fn seed_fails_hard_on_an_unknown_ingredient() {
    let (temp, pool) = TempDb::migrated("badseed").await;
    let bad = temp.dir.join("bad_seed.json");
    std::fs::write(
        &bad,
        r#"{
             "items": [
               {"code": "MN-TEST-01", "name": "Test", "group": "Coffee", "price": 1000, "uom": "Portion",
                "recipe": [{"item": "BB-TIDAK-ADA", "qty": 10, "uom": "Gram"}]}
             ],
             "raw_materials": [{"code": "BB-ADA", "name": "Ada", "uom": "Gram"}]
           }"#,
    )
    .unwrap();

    let error = seed::seed(&pool, &bad, false).await.expect_err("must fail");
    assert!(
        error.to_string().contains("unknown ingredient"),
        "error should name the broken recipe, got: {error}"
    );

    // A failed seed leaves nothing behind: the whole import is one transaction.
    let (items,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM items")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (recipes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recipes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((items, recipes), (0, 0), "rollback left rows behind");
}
