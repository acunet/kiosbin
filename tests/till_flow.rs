//! The money-and-stock path, end to end: money in, ingredients out, one transaction.

use std::path::{Path, PathBuf};

use kiosbin::{cart::Cart, db, import, sale, shift, stock, users};

struct Fixture {
    _dir: PathBuf,
    path: PathBuf,
    pool: sqlx::SqlitePool,
    settings: kiosbin::settings::Settings,
}

async fn fixture(tag: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("kiosbin-till-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("kiosbin.db");

    let pool = db::open(&path).await.expect("open");
    db::migrate(
        &pool,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"),
    )
    .await
    .expect("migrate");
    import::import_catalog(
        &pool,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/warkop3dara.seed.json"),
        false,
    )
    .await
    .expect("import");
    let settings = kiosbin::settings::load(&pool).await.expect("settings");
    Fixture {
        _dir: dir,
        path,
        pool,
        settings,
    }
}

async fn item_id(pool: &sqlx::SqlitePool, code: &str) -> i64 {
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM items WHERE code = ?")
        .bind(code)
        .fetch_one(pool)
        .await
        .expect("item exists");
    id
}

async fn moves(pool: &sqlx::SqlitePool, item_id: i64, reason: &str) -> i64 {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM stock_moves WHERE item_id = ? AND reason = ?")
            .bind(item_id)
            .bind(reason)
            .fetch_one(pool)
            .await
            .unwrap();
    count
}

#[tokio::test]
async fn a_cash_sale_charges_exactly_change_and_deducts_ingredients_once() {
    let f = fixture("cash").await;
    let shift_id = shift::open(&f.pool, "Rina", 100_000).await.expect("shift");

    // Kopi 3Dara (MN-KOPI-03, Rp 18.000) has 4 ingredients: susu, gula aren, cup, kopi.
    let kopi = item_id(&f.pool, "MN-KOPI-03").await;
    let susu = item_id(&f.pool, "BB-SUSU").await;
    stock::record(&f.pool, susu, 5_000.0, "opening", "opname-awal")
        .await
        .unwrap();

    let sale = sale::create(
        &f.pool,
        &f.settings,
        sale::SaleInput {
            cashier: "Rina".to_string(),
            shift_id,
            method: "Cash".to_string(),
            received: Some(50_000),
            discount: 0,
            lines: vec![(kopi, 2)],
        },
    )
    .await
    .expect("sale");

    assert_eq!(sale.subtotal, 36_000);
    assert_eq!(sale.total, 36_000);
    assert_eq!(sale.change, Some(14_000), "50.000 - 36.000");
    assert!(sale.number.starts_with("W3D-"), "number: {}", sale.number);
    assert_eq!(sale.stock_rows, 4, "one move per ingredient line");

    // Two cups -> 60 ml of milk (the recipe is 30 ml per serving), written once.
    let balance = stock::balance(&f.pool, susu).await.unwrap();
    assert_eq!(balance, 5_000.0 - 60.0, "milk: 30 ml per cup, 2 cups");
    assert_eq!(moves(&f.pool, susu, "sale").await, 1);

    // The sold menu item itself is not stock tracked (D9).
    assert_eq!(moves(&f.pool, kopi, "sale").await, 0);

    let (deducted,): (Option<String>,) =
        sqlx::query_as("SELECT deducted_at FROM sales WHERE id = ?")
            .bind(sale.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert!(deducted.is_some(), "sale must be stamped as deducted");
}

#[tokio::test]
async fn a_sale_number_follows_the_previous_one() {
    let f = fixture("numbers").await;
    let shift_id = shift::open(&f.pool, "Rina", 0).await.unwrap();
    let item = item_id(&f.pool, "MN-AD-TELUR").await;

    let mut numbers = Vec::new();
    for _ in 0..3 {
        let sale = sale::create(
            &f.pool,
            &f.settings,
            sale::SaleInput {
                cashier: "Rina".to_string(),
                shift_id,
                method: "Cash".to_string(),
                received: Some(5_000),
                discount: 0,
                lines: vec![(item, 1)],
            },
        )
        .await
        .expect("sale");
        numbers.push(sale.number);
    }
    assert!(numbers[0].ends_with("-001"), "{:?}", numbers);
    assert!(numbers[1].ends_with("-002"), "{:?}", numbers);
    assert!(numbers[2].ends_with("-003"), "{:?}", numbers);
}

#[tokio::test]
async fn the_till_refuses_to_sell_at_zero_or_below_the_received_cash() {
    let f = fixture("guards").await;
    let shift_id = shift::open(&f.pool, "Rina", 0).await.unwrap();
    let priced = item_id(&f.pool, "MN-AD-TELUR").await;
    let unpriced = item_id(&f.pool, "MN-KOPI-07").await; // price 0 in the live catalog

    let zero = sale::create(
        &f.pool,
        &f.settings,
        sale::SaleInput {
            cashier: "Rina".to_string(),
            shift_id,
            method: "Cash".to_string(),
            received: Some(10_000),
            discount: 0,
            lines: vec![(unpriced, 1)],
        },
    )
    .await
    .expect_err("a price of 0 must not sell silently");
    assert!(zero.to_string().contains("no price"), "{zero}");

    let short = sale::create(
        &f.pool,
        &f.settings,
        sale::SaleInput {
            cashier: "Rina".to_string(),
            shift_id,
            method: "Cash".to_string(),
            received: Some(1_000),
            discount: 0,
            lines: vec![(priced, 1)],
        },
    )
    .await
    .expect_err("cash below the total must be refused");
    assert!(short.to_string().contains("less than the total"), "{short}");

    let over_discount = sale::create(
        &f.pool,
        &f.settings,
        sale::SaleInput {
            cashier: "Rina".to_string(),
            shift_id,
            method: "Cash".to_string(),
            received: Some(50_000),
            discount: 99_000,
            lines: vec![(priced, 1)],
        },
    )
    .await
    .expect_err("a discount bigger than the bill must be refused");
    assert!(
        over_discount.to_string().contains("discount"),
        "{over_discount}"
    );

    // Nothing was written by any of the refused attempts.
    let (sales,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sales")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(sales, 0, "a refused sale must not leave a receipt behind");
}

#[tokio::test]
async fn qris_never_lands_in_the_expected_cash() {
    let f = fixture("qris").await;
    let shift_id = shift::open(&f.pool, "Rina", 200_000).await.unwrap();
    let item = item_id(&f.pool, "MN-AD-TELUR").await;

    sale::create(
        &f.pool,
        &f.settings,
        sale::SaleInput {
            cashier: "Rina".to_string(),
            shift_id,
            method: "QRIS".to_string(),
            received: None,
            discount: 0,
            lines: vec![(item, 2)], // 2 x 5.000
        },
    )
    .await
    .expect("qris sale");

    let shift_row = shift::by_id(&f.pool, shift_id).await.unwrap().unwrap();
    let expected = shift::expected(&f.pool, &shift_row, &f.settings)
        .await
        .unwrap();

    let cash = expected.iter().find(|e| e.method == "Cash").unwrap();
    let qris = expected.iter().find(|e| e.method == "QRIS").unwrap();
    assert_eq!(cash.expected, 200_000, "cash expected is just the float");
    assert_eq!(qris.expected, 10_000, "qris expected is its own takings");
}

#[tokio::test]
async fn a_blank_count_is_refused_and_a_typed_zero_is_recorded() {
    let f = fixture("count").await;
    let shift_id = shift::open(&f.pool, "Rina", 100_000).await.unwrap();
    let item = item_id(&f.pool, "MN-AD-TELUR").await;
    sale::create(
        &f.pool,
        &f.settings,
        sale::SaleInput {
            cashier: "Rina".to_string(),
            shift_id,
            method: "Cash".to_string(),
            received: Some(10_000),
            discount: 0,
            lines: vec![(item, 1)],
        },
    )
    .await
    .expect("sale");

    let shift_row = shift::by_id(&f.pool, shift_id).await.unwrap().unwrap();

    // Left empty -> refused: that is the whole point of the reconciliation rule.
    let blank = shift::close(
        &f.pool,
        &shift_row,
        &f.settings,
        &[("Cash".to_string(), None), ("QRIS".to_string(), Some(0))],
    )
    .await
    .expect_err("an empty count must not close the shift");
    assert!(
        blank.to_string().contains("masukkan jumlah fisik"),
        "{blank}"
    );

    // Typed 0 -> accepted, and the difference shows the drawer really is short.
    let lines = shift::close(
        &f.pool,
        &shift_row,
        &f.settings,
        &[("Cash".to_string(), Some(0)), ("QRIS".to_string(), Some(0))],
    )
    .await
    .expect("close");
    let cash = lines.iter().find(|l| l.method == "Cash").unwrap();
    assert_eq!(cash.expected, 105_000, "100.000 float + 5.000 cash sale");
    assert_eq!(cash.diff, -105_000, "counted 0 against expected cash");

    let (status,): (String,) = sqlx::query_as("SELECT status FROM shifts WHERE id = ?")
        .bind(shift_id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(status, "closed");
}

#[tokio::test]
async fn voiding_puts_the_ingredients_back_and_keeps_the_receipt() {
    let f = fixture("void").await;
    let shift_id = shift::open(&f.pool, "Rina", 0).await.unwrap();
    let kopi = item_id(&f.pool, "MN-KOPI-03").await;
    let susu = item_id(&f.pool, "BB-SUSU").await;

    stock::record(&f.pool, susu, 1_000.0, "opening", "opening-awal")
        .await
        .unwrap();
    let sale = sale::create(
        &f.pool,
        &f.settings,
        sale::SaleInput {
            cashier: "Rina".to_string(),
            shift_id,
            method: "Cash".to_string(),
            received: Some(20_000),
            discount: 0,
            lines: vec![(kopi, 1)],
        },
    )
    .await
    .expect("sale");
    assert_eq!(
        stock::balance(&f.pool, susu).await.unwrap(),
        970.0,
        "30 ml for one cup"
    );

    sale::void(&f.pool, sale.id, "salah pesan")
        .await
        .expect("void");

    assert_eq!(
        stock::balance(&f.pool, susu).await.unwrap(),
        1_000.0,
        "stock returned"
    );
    let (status,): (String,) = sqlx::query_as("SELECT status FROM sales WHERE id = ?")
        .bind(sale.id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(status, "void", "receipt kept, only its status changed");

    // The voided sale stops counting towards the shift.
    let shift_row = shift::by_id(&f.pool, shift_id).await.unwrap().unwrap();
    let expected = shift::expected(&f.pool, &shift_row, &f.settings)
        .await
        .unwrap();
    let cash = expected.iter().find(|e| e.method == "Cash").unwrap();
    assert_eq!(cash.expected, 0);
}

#[tokio::test]
async fn a_cart_keeps_item_ids_only() {
    let mut cart = Cart::default();
    cart.add(1, 2);
    assert_eq!(cart.total_qty(), 2);
    assert_eq!(cart.lines[0].item_id, 1);
}

#[tokio::test]
async fn a_pin_is_checked_against_its_hash() {
    let f = fixture("pin").await;
    users::create(&f.pool, "Rina", "1234", "cashier")
        .await
        .expect("user");

    assert!(users::verify(&f.pool, "Rina", "1234")
        .await
        .unwrap()
        .is_some());
    assert!(users::verify(&f.pool, "Rina", "4321")
        .await
        .unwrap()
        .is_none());
    assert!(users::verify(&f.pool, "Tidak Ada", "1234")
        .await
        .unwrap()
        .is_none());

    // The PIN itself is never stored.
    let (salt, hash): (String, String) =
        sqlx::query_as("SELECT pin_salt, pin_hash FROM users WHERE name = 'Rina'")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_ne!(salt, "1234");
    assert_ne!(hash, "1234");
    assert_eq!(hash.len(), 64, "sha256 hex");

    let _ = f.path;
}
