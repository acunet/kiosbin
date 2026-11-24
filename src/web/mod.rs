//! HTTP layer: one binary serves the till UI. Server-rendered with HTMX, so there is no
//! separate frontend build and no API version drift.

pub mod shift;
pub mod till;

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

use crate::cart::Cart;
use crate::settings::{BoxError, Result, Settings};

pub type BoxErr = BoxError;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
}

impl AppState {
    /// Settings are read per request: one tiny query, and the owner's edits to printer or
    /// currency take effect without a restart.
    pub async fn settings(&self) -> Result<Settings> {
        crate::settings::load(&self.pool).await
    }
}

pub fn router(pool: SqlitePool) -> Router {
    let store = MemoryStore::default();
    // Plain http on the shop LAN: no TLS, so the cookie cannot be marked secure.
    let session_layer = SessionManagerLayer::new(store).with_secure(false);

    Router::new()
        .route("/health", get(health))
        .route("/assets/htmx.min.js", get(htmx_js))
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .route("/", get(till::till))
        .route("/items", get(till::items_partial))
        .route("/cart", get(till::cart_partial))
        .route("/cart/add", post(till::cart_add))
        .route("/cart/qty", post(till::cart_qty))
        .route("/cart/remove", post(till::cart_remove))
        .route("/cart/clear", post(till::cart_clear))
        .route("/pay", get(till::pay_page).post(till::pay))
        .route("/receipt/{id}", get(till::receipt))
        .route("/shift/open", get(shift::open_page).post(shift::open))
        .route("/shift/close", get(shift::close_page).post(shift::close))
        .with_state(AppState { pool })
        .layer(session_layer)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

/// HTMX is vendored into the binary (assets/README.md records the version and license), so a
/// shop install is still exactly one file and the till needs no CDN on the LAN.
async fn htmx_js() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../assets/htmx.min.js"),
    )
}

// ---------------------------------------------------------------------------
// Session helpers
// ---------------------------------------------------------------------------

pub const KEY_CASHIER: &str = "cashier";
pub const KEY_ROLE: &str = "role";
pub const KEY_CART: &str = "cart";
pub const KEY_FLASH: &str = "flash";
pub const KEY_FAILS: &str = "login_fails";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Whoami {
    pub cashier: String,
    pub role: String,
}

pub async fn whoami(session: &Session) -> Option<Whoami> {
    let cashier: Option<String> = session.get(KEY_CASHIER).await.ok().flatten();
    let cashier = cashier?;
    let role: Option<String> = session.get(KEY_ROLE).await.ok().flatten();
    Some(Whoami {
        cashier,
        role: role.unwrap_or_else(|| "cashier".to_string()),
    })
}

pub async fn take_flash(session: &Session) -> String {
    let flash: Option<String> = session.get(KEY_FLASH).await.ok().flatten();
    if flash.is_some() {
        let _ = session.remove::<String>(KEY_FLASH).await;
    }
    flash.unwrap_or_default()
}

pub async fn set_flash(session: &Session, message: impl Into<String>) {
    let _ = session.insert(KEY_FLASH, message.into()).await;
}

pub async fn load_cart(session: &Session) -> Cart {
    session
        .get::<Cart>(KEY_CART)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub async fn save_cart(session: &Session, cart: &Cart) {
    let _ = session.insert(KEY_CART, cart.clone()).await;
}

/// Renders an Askama template, turning a template error into a visible 500 instead of a panic.
pub fn render<T: Template>(template: T) -> Response {
    match template.render() {
        Ok(body) => Html(body).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("template error: {e}"),
        )
            .into_response(),
    }
}

pub fn bad_request(message: impl Into<String>) -> Response {
    (StatusCode::BAD_REQUEST, message.into()).into_response()
}

pub fn redirect(to: &str) -> Response {
    Redirect::to(to).into_response()
}

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub outlet_name: String,
    pub cashiers: Vec<crate::users::Cashier>,
    pub flash: String,
}

async fn login_page(State(state): State<AppState>, session: Session) -> Response {
    let settings = match state.settings().await {
        Ok(s) => s,
        Err(e) => return bad_request(e.to_string()),
    };
    let cashiers = crate::users::list(&state.pool).await.unwrap_or_default();
    if cashiers.is_empty() {
        return bad_request(
            "no cashier yet — create one with: kiosbin user add <name> --pin <pin>",
        );
    }
    render(LoginTemplate {
        outlet_name: settings.outlet_name(),
        cashiers,
        flash: take_flash(&session).await,
    })
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub cashier: String,
    pub pin: String,
}

async fn login(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> Response {
    let fails: i64 = session
        .get::<i64>(KEY_FAILS)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);
    if fails >= 10 {
        return redirect("/login");
    }

    match crate::users::verify(&state.pool, &form.cashier, form.pin.trim()).await {
        Ok(Some(user)) => {
            let _ = session.insert(KEY_CASHIER, user.name.clone()).await;
            let _ = session.insert(KEY_ROLE, user.role.clone()).await;
            let _ = session.insert(KEY_CART, Cart::default()).await;
            let _ = session.remove::<i64>(KEY_FAILS).await;
            redirect("/")
        }
        Ok(None) => {
            let _ = session.insert(KEY_FAILS, fails + 1).await;
            // Slow a guessing loop down without making a typo painful.
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            set_flash(&session, "PIN salah").await;
            redirect("/login")
        }
        Err(e) => bad_request(e.to_string()),
    }
}

async fn logout(session: Session) -> Response {
    let _ = session.flush().await;
    redirect("/login")
}

// ---------------------------------------------------------------------------
// Shared view models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ItemTile {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub group_name: String,
    pub price_display: String,
    pub no_price: bool,
}

#[derive(Debug, Clone)]
pub struct CartLineView {
    pub item_id: i64,
    pub name: String,
    pub qty: i64,
    pub unit_price_display: String,
    pub amount_display: String,
}

#[derive(Debug, Clone, Default)]
pub struct CartView {
    pub lines: Vec<CartLineView>,
    pub subtotal: i64,
    pub subtotal_display: String,
    pub total_qty: i64,
    pub empty: bool,
}

/// Turns the session cart into something a template can print, re-reading names and prices
/// from the database so the displayed price is always the current one.
pub async fn cart_view(pool: &SqlitePool, settings: &Settings, cart: &Cart) -> Result<CartView> {
    let mut lines = Vec::with_capacity(cart.lines.len());
    let mut subtotal = 0i64;
    for line in &cart.lines {
        let item = crate::catalog::by_id(pool, line.item_id).await?;
        let Some(item) = item else { continue };
        let amount = item.price * line.qty;
        subtotal += amount;
        lines.push(CartLineView {
            item_id: item.id,
            name: item.name,
            qty: line.qty,
            unit_price_display: settings.money(item.price),
            amount_display: settings.money(amount),
        });
    }
    Ok(CartView {
        empty: lines.is_empty(),
        lines,
        subtotal,
        subtotal_display: settings.money(subtotal),
        total_qty: cart.total_qty(),
    })
}

#[derive(Template)]
#[template(path = "partials/cart.html")]
pub struct CartPartial {
    pub cart: CartView,
}

#[derive(Deserialize)]
pub struct TillQuery {
    pub group: Option<String>,
    pub q: Option<String>,
}

pub async fn group_tabs(pool: &SqlitePool) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT group_name FROM items
         WHERE kind IN ('menu', 'addon', 'service') AND active = 1 AND group_name <> ''
         ORDER BY group_name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(g,)| g).collect())
}

pub fn tiles(items: Vec<crate::catalog::Item>, settings: &Settings) -> Vec<ItemTile> {
    items
        .into_iter()
        .map(|item| ItemTile {
            id: item.id,
            code: item.code,
            name: item.name,
            group_name: item.group_name,
            price_display: settings.money(item.price),
            no_price: item.price <= 0,
        })
        .collect()
}

pub type QueryMap = HashMap<String, String>;

pub fn query_value(query: &Query<TillQuery>, key: &str) -> String {
    match key {
        "q" => query.q.clone().unwrap_or_default(),
        "group" => query.group.clone().unwrap_or_default(),
        _ => String::new(),
    }
}
