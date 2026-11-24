//! End-to-end checks for what the money and stock paths depend on: migrating from an
//! empty file, the constraints the schema promises, and catalog import behaviour.

use std::path::{Path, PathBuf};

use kiosbin::{db, import};

struct TempDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TempDb {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kiosbin-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("kiosbin.db");
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

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn migrations_dir() -> PathBuf {
    repo_file("migrations")
}

/// The reference tenant fixture (the ERPNext shop this product was extracted from).
fn tenant_fixture() -> PathBuf {
    repo_file("fixtures/warkop3dara.seed.json")
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
        sqlx::query("INSERT INTO stock_moves (item_id, qty, reason, at) VALUES (9999, 1, 'opening', '2026-11-24 08:00:00')")
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
    let open_shift = "INSERT INTO shifts (cashier, opened_at, opening_float) VALUES (?, ?, ?)";

    sqlx::query(open_shift)
        .bind("Rina")
        .bind("2026-11-24 08:00:00")
        .bind(100_000)
        .execute(&pool)
        .await
        .expect("first open shift");

    let second = sqlx::query(open_shift)
        .bind("Rina")
        .bind("2026-11-24 15:00:00")
        .bind(50_000)
        .execute(&pool)
        .await;
    assert!(
        second.is_err(),
        "partial unique index must block a second open shift"
    );

    // A different cashier is still allowed, and a closed shift never blocks.
    sqlx::query(open_shift)
        .bind("Dewi")
        .bind("2026-11-24 15:00:00")
        .bind(50_000)
        .execute(&pool)
        .await
        .expect("other cashier");

    sqlx::query("UPDATE shifts SET status = 'closed', closed_at = '2026-11-24 22:00:00' WHERE cashier = 'Rina'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(open_shift)
        .bind("Rina")
        .bind("2026-11-25 08:00:00")
        .bind(20_000)
        .execute(&pool)
        .await
        .expect("new shift after closing the previous one");
}

#[tokio::test]
async fn imports_the_reference_tenant_catalogue() {
    let (_temp, pool) = TempDb::migrated("import").await;

    let first = import::import_catalog(&pool, &tenant_fixture(), false)
        .await
        .expect("import");
    assert_eq!(
        first.items_written, 83,
        "39 sellable items + 44 raw materials"
    );

    let (items,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM items")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (recipes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recipes")
        .fetch_one(&pool)
        .await
        .unwrap();
    let kinds: Vec<(String, i64)> =
        sqlx::query_as("SELECT kind, COUNT(*) FROM items GROUP BY kind ORDER BY kind")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(items, 83);
    assert_eq!(
        recipes, 70,
        "35 recipes / 70 ingredient lines in the fixture"
    );
    assert_eq!(
        kinds,
        vec![
            ("addon".to_string(), 6),
            ("menu".to_string(), 32),
            ("raw".to_string(), 44),
            ("service".to_string(), 1),
        ]
    );

    // Outlet settings come from the fixture, not from the code.
    let outlet: (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'outlet_name'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let prefix: (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'invoice_prefix'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(outlet.0, "Warkop 3Dara");
    assert_eq!(prefix.0, "W3D");
}

#[tokio::test]
async fn importing_twice_changes_nothing() {
    let (_temp, pool) = TempDb::migrated("twice").await;
    import::import_catalog(&pool, &tenant_fixture(), false)
        .await
        .expect("first");

    let second = import::import_catalog(&pool, &tenant_fixture(), false)
        .await
        .expect("second");
    assert_eq!(
        second.items_written, 0,
        "re-import must not insert anything new"
    );
    assert_eq!(second.items_kept, 83);
    assert_eq!(second.recipes_written, 0);
    assert_eq!(second.settings_added, 0);

    let (recipes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recipes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(recipes, 70, "no duplicate recipe lines");
}

#[tokio::test]
async fn import_keeps_prices_edited_inside_the_pos() {
    let (_temp, pool) = TempDb::migrated("price").await;
    import::import_catalog(&pool, &tenant_fixture(), false)
        .await
        .expect("import");

    sqlx::query("UPDATE items SET price = 21000 WHERE code = 'MN-KOPI-03'")
        .execute(&pool)
        .await
        .unwrap();

    import::import_catalog(&pool, &tenant_fixture(), false)
        .await
        .expect("re-import");
    let (price,): (i64,) = sqlx::query_as("SELECT price FROM items WHERE code = 'MN-KOPI-03'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        price, 21_000,
        "a plain import must not revert a manual price change"
    );

    import::import_catalog(&pool, &tenant_fixture(), true)
        .await
        .expect("forced import");
    let (forced,): (i64,) = sqlx::query_as("SELECT price FROM items WHERE code = 'MN-KOPI-03'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(forced, 18_000, "--force re-reads the catalog price");

    // A forced import must still not clobber the outlet settings the owner configured.
    let (outlet,): (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'outlet_name'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(outlet, "Warkop 3Dara");
}

#[tokio::test]
async fn import_fails_hard_on_an_unknown_ingredient_and_rolls_back() {
    let (temp, pool) = TempDb::migrated("bad").await;
    let bad = temp.dir.join("bad.seed.json");
    std::fs::write(
        &bad,
        r#"{
             "items": [
               {"code": "SKU-1", "name": "Test", "group": "Coffee", "kind": "menu", "price": 1000,
                "uom": "Portion", "recipe": [{"item": "RAW-MISSING", "qty": 10, "uom": "Gram"}]}
             ],
             "raw_materials": [{"code": "RAW-1", "name": "Present", "uom": "Gram"}]
           }"#,
    )
    .unwrap();

    let error = import::import_catalog(&pool, &bad, false)
        .await
        .expect_err("must fail");
    assert!(
        error.to_string().contains("unknown ingredient"),
        "error should name the broken recipe, got: {error}"
    );

    // A failed import leaves nothing behind: the whole thing is one transaction.
    let (items,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM items")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (settings,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM settings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((items, settings), (0, 0), "rollback left rows behind");
}

#[tokio::test]
async fn a_minimal_fixture_gets_the_product_defaults() {
    let (temp, pool) = TempDb::migrated("minimal").await;
    let minimal = temp.dir.join("minimal.seed.json");
    std::fs::write(
        &minimal,
        r#"{"items": [{"code": "SKU-1", "name": "Kopi", "price": 5000}]}"#,
    )
    .unwrap();

    import::import_catalog(&pool, &minimal, false)
        .await
        .expect("import");

    let (name,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'outlet_name'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (prefix,): (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'invoice_prefix'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let (kind,): (String,) = sqlx::query_as("SELECT kind FROM items WHERE code = 'SKU-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        name, "My Outlet",
        "no outlet section in the fixture -> product default"
    );
    assert_eq!(prefix, "POS");
    assert_eq!(
        kind, "menu",
        "an item with no kind and no matching rule is a menu item"
    );
}

#[tokio::test]
async fn fixture_kind_wins_over_the_inference_rules() {
    let (temp, pool) = TempDb::migrated("kind").await;
    let file = temp.dir.join("kind.seed.json");
    // BB- would normally be inferred as raw, but the fixture says otherwise.
    std::fs::write(
        &file,
        r#"{"items": [{"code": "BB-1", "name": "Mislabeled", "kind": "addon", "price": 3000}]}"#,
    )
    .unwrap();

    import::import_catalog(&pool, &file, false)
        .await
        .expect("import");
    let (kind,): (String,) = sqlx::query_as("SELECT kind FROM items WHERE code = 'BB-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kind, "addon");

    // A kind that is not part of the product's vocabulary is rejected loudly.
    let bad = temp.dir.join("kind-bad.seed.json");
    std::fs::write(
        &bad,
        r#"{"items": [{"code": "X", "name": "X", "kind": "chicken"}]}"#,
    )
    .unwrap();
    let error = import::import_catalog(&pool, &bad, false)
        .await
        .expect_err("must fail");
    assert!(
        error.to_string().contains("expected one of"),
        "got: {error}"
    );
}
