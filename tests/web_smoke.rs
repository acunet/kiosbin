//! HTTP smoke test: the real router, real sessions, no browser.
//!
//! It drives exactly the path a cashier drives — login, open a shift, add an item, pay cash,
//! read the receipt — and checks the status codes and the rendered pages.

use std::path::{Path, PathBuf};

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use kiosbin::{db, import, users, web};
use tower::ServiceExt;

struct App {
    _dir: PathBuf,
    router: axum::Router,
    pool: sqlx::SqlitePool,
}

async fn app(tag: &str) -> App {
    let dir = std::env::temp_dir().join(format!("kiosbin-web-{tag}-{}", std::process::id()));
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
    users::create(&pool, "Rina", "1234", "cashier")
        .await
        .expect("cashier");

    App {
        _dir: dir,
        router: web::router(pool.clone()),
        pool,
    }
}

async fn item_id(app: &App, code: &str) -> i64 {
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM items WHERE code = ?")
        .bind(code)
        .fetch_one(&app.pool)
        .await
        .expect("item");
    id
}

fn form(body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn get(app: &App, uri: &str, cookie: Option<&str>) -> (StatusCode, String, Option<String>) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .router
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .expect("response");

    let status = response.status();
    let session = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next().map(str::to_string));
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&body).to_string(), session)
}

async fn post(
    app: &App,
    uri: &str,
    body: &str,
    cookie: Option<&str>,
) -> (StatusCode, String, Option<String>) {
    let mut request = form(body);
    *request.uri_mut() = uri.parse().unwrap();
    if let Some(cookie) = cookie {
        request
            .headers_mut()
            .insert(header::COOKIE, cookie.parse().unwrap());
    }
    let response = app.router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let session = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next().map(str::to_string));
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&body).to_string(), session)
}

#[tokio::test]
async fn health_is_open_and_everything_else_needs_a_login() {
    let app = app("health").await;

    let (status, body, _) = get(&app, "/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"ok\":true"));

    let (status, _, _) = get(&app, "/", None).await;
    assert!(
        status.is_redirection(),
        "the till must not open without a login"
    );

    let (status, body, _) = get(&app, "/login", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Rina"), "cashiers are listed: {body:.200}");

    // A session only starts when there is something to keep in it.
    let (status, _, cookie) = post(&app, "/login", "cashier=Rina&pin=1234", None).await;
    assert!(status.is_redirection());
    assert!(cookie.is_some(), "a session cookie is issued on login");
}

#[tokio::test]
async fn a_wrong_pin_does_not_get_in_and_a_right_one_does() {
    let app = app("login").await;
    let (_, _, cookie) = get(&app, "/login", None).await;

    let (status, _, _) = post(&app, "/login", "cashier=Rina&pin=9999", cookie.as_deref()).await;
    assert!(status.is_redirection());

    let (status, body, _) = get(&app, "/", cookie.as_deref()).await;
    assert!(status.is_redirection(), "still locked out");
    assert!(body.is_empty() || !body.contains("Keranjang"));

    let (status, _, cookie) =
        post(&app, "/login", "cashier=Rina&pin=1234", cookie.as_deref()).await;
    assert!(status.is_redirection(), "successful login redirects");

    // No shift yet: the till sends the cashier to open one.
    let (status, _, _) = get(&app, "/", cookie.as_deref()).await;
    assert!(status.is_redirection());

    let (status, body, _) = get(&app, "/shift/open", cookie.as_deref()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Buka Shift"));
}

#[tokio::test]
async fn a_full_till_run_from_login_to_receipt() {
    let app = app("flow").await;
    let (_, _, cookie) = get(&app, "/login", None).await;
    let (_, _, cookie) = post(&app, "/login", "cashier=Rina&pin=1234", cookie.as_deref()).await;
    let cookie = cookie.as_deref();

    // 1. open the shift with a float
    let (status, _, _) = post(&app, "/shift/open", "opening_float=100.000", cookie).await;
    assert!(status.is_redirection());

    // 2. the grid renders the catalog
    let (status, body, _) = get(&app, "/", cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Kopi 3Dara"), "the menu is on the till");
    assert!(body.contains("Keranjang"));

    // 3. search finds a single item
    let (status, body, _) = get(&app, "/items?q=kopi", cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("MN-KOPI-03"), "{body:.300}");

    // 4. put it in the cart twice through the HTMX endpoint
    let kopi = item_id(&app, "MN-KOPI-03").await;
    let (status, body, _) = post(&app, "/cart/add", &format!("item_id={kopi}"), cookie).await;
    assert_eq!(status, StatusCode::OK, "cart add: {body}");
    let (status, body, _) = post(&app, "/cart/add", &format!("item_id={kopi}"), cookie).await;
    assert_eq!(status, StatusCode::OK, "cart add again: {body}");
    assert!(body.contains("Keranjang"), "the cart partial comes back");
    assert!(body.contains("Rp 36.000"), "2 x 18.000 in the cart: {body}");

    // 5. pay cash and land on the receipt
    let (status, body, _) = post(
        &app,
        "/pay",
        "method=Cash&received=50.000&discount=0",
        cookie,
    )
    .await;
    assert!(
        status.is_redirection(),
        "a paid sale redirects to its receipt: {body}"
    );

    let (status, body, _) = get(&app, "/receipt/1", cookie).await;
    assert_eq!(status, StatusCode::OK, "receipt: {body}");
    assert!(
        body.contains("W3D-"),
        "the receipt shows its number: {body:.300}"
    );
    assert!(
        body.contains("Rp 14.000"),
        "50.000 - 36.000 change: {body:.400}"
    );
    assert!(body.contains("paper"), "the ESC/POS preview is on the page");

    // 6. close the shift: the form wants a physical count per method
    let (status, body, _) = get(&app, "/shift/close", cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Hitung fisik"));

    let (status, body, _) = post(
        &app,
        "/shift/close",
        "counted_Cash=105.000&counted_QRIS=0",
        cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Shift ditutup"), "{body:.300}");
    assert!(
        body.contains("Rp 105.000"),
        "expected cash is shown: {body:.400}"
    );
    assert!(
        body.contains("Rp 0"),
        "the counted box was typed, not auto-filled"
    );
}
