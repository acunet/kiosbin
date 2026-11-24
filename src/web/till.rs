//! The till: item grid, cart, payment, receipt.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Form;
use serde::Deserialize;
use tower_sessions::Session;

use crate::sale::{self, SaleInput};
use crate::settings::Settings;
use crate::web::{
    bad_request, cart_view, group_tabs, load_cart, redirect, render, save_cart, set_flash,
    take_flash, tiles, whoami, AppState, CartPartial, TillQuery,
};

#[derive(Debug, Clone)]
struct GroupTab {
    key: String,
    label: String,
    active: bool,
}

#[derive(Template)]
#[template(path = "till.html")]
struct TillTemplate {
    outlet_name: String,
    cashier: String,
    opened_at: String,
    groups: Vec<GroupTab>,
    query: String,
    items: Vec<crate::web::ItemTile>,
    cart: crate::web::CartView,
    today_count: i64,
    today_total_display: String,
    low_stock: i64,
    flash: String,
}

#[derive(Template)]
#[template(path = "partials/items.html")]
struct ItemsPartial {
    items: Vec<crate::web::ItemTile>,
}

async fn grid_items(
    state: &AppState,
    query: &TillQuery,
) -> Result<Vec<crate::catalog::Item>, crate::settings::BoxError> {
    let items = match (&query.q, &query.group) {
        (Some(q), _) if !q.trim().is_empty() => crate::catalog::search(&state.pool, q).await?,
        (_, Some(group)) if !group.is_empty() && group != "all" => {
            crate::catalog::sellable(&state.pool)
                .await?
                .into_iter()
                .filter(|i| &i.group_name == group)
                .collect()
        }
        _ => crate::catalog::sellable(&state.pool).await?,
    };
    Ok(items)
}

pub async fn till(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<TillQuery>,
) -> Response {
    let Some(_user) = whoami(&session).await else {
        return redirect("/login");
    };
    let settings = match state.settings().await {
        Ok(s) => s,
        Err(e) => return bad_request(e.to_string()),
    };
    let cart = load_cart(&session).await;

    match render_till(&state, &session, &settings, &query, &cart).await {
        Ok(response) => response,
        Err(e) => bad_request(e.to_string()),
    }
}

async fn render_till(
    state: &AppState,
    session: &Session,
    settings: &Settings,
    query: &TillQuery,
    cart: &crate::cart::Cart,
) -> Result<Response, crate::settings::BoxError> {
    let Some(user) = whoami(session).await else {
        return Ok(redirect("/login"));
    };
    let Some(shift) = crate::shift::open_for(&state.pool, &user.cashier).await? else {
        return Ok(redirect("/shift/open"));
    };

    let items = grid_items(state, query).await?;
    let cart_view = cart_view(&state.pool, settings, cart).await?;
    let (today_count, today_total) =
        sale::today_totals(&state.pool, &settings.invoice_prefix()).await?;
    let low_stock = crate::stock::items_at_or_below_zero(&state.pool)
        .await?
        .len() as i64;
    let active_group = query.group.clone().unwrap_or_else(|| "all".to_string());

    let mut groups = vec![GroupTab {
        key: "all".to_string(),
        label: "Semua".to_string(),
        active: active_group == "all",
    }];
    for name in group_tabs(&state.pool).await? {
        groups.push(GroupTab {
            active: name == active_group,
            key: name.clone(),
            label: name,
        });
    }

    Ok(render(TillTemplate {
        outlet_name: settings.outlet_name(),
        cashier: user.cashier,
        opened_at: shift
            .opened_at
            .get(..16)
            .unwrap_or(&shift.opened_at)
            .to_string(),
        groups,
        query: query.q.clone().unwrap_or_default(),
        items: tiles(items, settings),
        cart: cart_view,
        today_count,
        today_total_display: settings.money(today_total),
        low_stock,
        flash: take_flash(session).await,
    }))
}

pub async fn items_partial(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<TillQuery>,
) -> Response {
    if whoami(&session).await.is_none() {
        return redirect("/login");
    }
    let settings = match state.settings().await {
        Ok(s) => s,
        Err(e) => return bad_request(e.to_string()),
    };
    match grid_items(&state, &query).await {
        Ok(items) => render(ItemsPartial {
            items: tiles(items, &settings),
        }),
        Err(e) => bad_request(e.to_string()),
    }
}

pub async fn cart_partial(State(state): State<AppState>, session: Session) -> Response {
    let settings = match state.settings().await {
        Ok(s) => s,
        Err(e) => return bad_request(e.to_string()),
    };
    let cart = load_cart(&session).await;
    match cart_view(&state.pool, &settings, &cart).await {
        Ok(view) => render(CartPartial { cart: view }),
        Err(e) => bad_request(e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct ItemForm {
    pub item_id: i64,
}

#[derive(Deserialize)]
pub struct QtyForm {
    pub item_id: i64,
    pub qty: i64,
}

pub async fn cart_add(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<ItemForm>,
) -> Response {
    if whoami(&session).await.is_none() {
        return redirect("/login");
    }
    let mut cart = load_cart(&session).await;
    cart.add(form.item_id, 1);
    save_cart(&session, &cart).await;
    cart_partial(State(state), session).await
}

pub async fn cart_qty(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<QtyForm>,
) -> Response {
    let mut cart = load_cart(&session).await;
    cart.set_qty(form.item_id, form.qty);
    save_cart(&session, &cart).await;
    cart_partial(State(state), session).await
}

pub async fn cart_remove(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<ItemForm>,
) -> Response {
    let mut cart = load_cart(&session).await;
    cart.remove(form.item_id);
    save_cart(&session, &cart).await;
    cart_partial(State(state), session).await
}

pub async fn cart_clear(State(state): State<AppState>, session: Session) -> Response {
    let cart = crate::cart::Cart::default();
    save_cart(&session, &cart).await;
    cart_partial(State(state), session).await
}

// ---------------------------------------------------------------------------
// Payment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MethodOption {
    name: String,
    is_cash: bool,
    checked: bool,
}

#[derive(Debug, Clone)]
struct QuickAmount {
    label: String,
    value: i64,
}

#[derive(Debug, Clone)]
struct LineDisplay {
    name: String,
    qty: i64,
    unit: String,
    amount: String,
}

#[derive(Template)]
#[template(path = "pay.html")]
struct PayTemplate {
    outlet_name: String,
    cashier: String,
    cart: crate::web::CartView,
    total: i64,
    total_display: String,
    methods: Vec<MethodOption>,
    quick_amounts: Vec<QuickAmount>,
    flash: String,
}

/// Cash buttons a real till needs: the exact amount, then round notes above the total.
fn quick_amounts(total: i64) -> Vec<(String, i64)> {
    let mut values = vec![total];
    for step in [5_000i64, 10_000, 50_000, 100_000] {
        let rounded = total.div_euclid(step) * step + if total % step == 0 { 0 } else { step };
        if !values.contains(&rounded) {
            values.push(rounded);
        }
    }
    values.sort_unstable();
    values
        .into_iter()
        .take(6)
        .map(|v| (format!("{v}"), v))
        .collect()
}

async fn pay_context(
    state: &AppState,
    session: &Session,
) -> Result<(Settings, crate::web::Whoami, crate::web::CartView), crate::settings::BoxError> {
    let settings = state.settings().await?;
    let user = whoami(session).await.ok_or("not logged in")?;
    let cart = load_cart(session).await;
    let view = cart_view(&state.pool, &settings, &cart).await?;
    Ok((settings, user, view))
}

pub async fn pay_page(State(state): State<AppState>, session: Session) -> Response {
    let (settings, user, cart) = match pay_context(&state, &session).await {
        Ok(v) => v,
        Err(_) => return redirect("/login"),
    };
    if cart.empty {
        return redirect("/");
    }
    let cash_method = settings.cash_method();
    render(PayTemplate {
        outlet_name: settings.outlet_name(),
        cashier: user.cashier,
        total: cart.subtotal,
        total_display: cart.subtotal_display.clone(),
        methods: settings
            .payment_methods()
            .into_iter()
            .enumerate()
            .map(|(i, name)| MethodOption {
                is_cash: name == cash_method,
                checked: i == 0,
                name,
            })
            .collect(),
        quick_amounts: quick_amounts(cart.subtotal)
            .into_iter()
            .map(|(_, value)| QuickAmount {
                label: settings.money(value),
                value,
            })
            .collect(),
        cart,
        flash: take_flash(&session).await,
    })
}

#[derive(Deserialize)]
pub struct PayForm {
    pub method: String,
    #[serde(default)]
    pub received: String,
    #[serde(default)]
    pub discount: String,
}

pub async fn pay(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<PayForm>,
) -> Response {
    let (settings, user, _) = match pay_context(&state, &session).await {
        Ok(v) => v,
        Err(_) => return redirect("/login"),
    };
    let Some(shift) = crate::shift::open_for(&state.pool, &user.cashier)
        .await
        .ok()
        .flatten()
    else {
        return redirect("/shift/open");
    };

    let cart = load_cart(&session).await;
    let discount = crate::money::parse(&form.discount).unwrap_or(0);
    let received = crate::money::parse(&form.received);

    let input = SaleInput {
        cashier: user.cashier.clone(),
        shift_id: shift.id,
        method: form.method.clone(),
        received,
        discount,
        lines: cart.lines.iter().map(|l| (l.item_id, l.qty)).collect(),
    };

    match sale::create(&state.pool, &settings, input).await {
        Ok(result) => {
            save_cart(&session, &crate::cart::Cart::default()).await;
            redirect(&format!("/receipt/{}", result.id))
        }
        Err(e) => {
            set_flash(&session, e.to_string()).await;
            redirect("/pay")
        }
    }
}

#[derive(Template)]
#[template(path = "receipt.html")]
struct ReceiptTemplate {
    outlet_name: String,
    cashier: String,
    sale: crate::sale::SaleResult,
    total_display: String,
    subtotal_display: String,
    discount_display: String,
    received_display: String,
    change_display: String,
    is_cash: bool,
    lines_display: Vec<LineDisplay>,
    paper_preview: String,
    paper_cols: usize,
    low_stock: Vec<String>,
    flash: String,
}

pub async fn receipt(
    State(state): State<AppState>,
    session: Session,
    Path(sale_id): Path<i64>,
) -> Response {
    let Some(_user) = whoami(&session).await else {
        return redirect("/login");
    };
    let settings = match state.settings().await {
        Ok(s) => s,
        Err(e) => return bad_request(e.to_string()),
    };
    let sale = match sale::by_id(&state.pool, sale_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return bad_request("nota tidak ditemukan"),
        Err(e) => return bad_request(e.to_string()),
    };
    let low_stock = crate::stock::items_at_or_below_zero(&state.pool)
        .await
        .map(|map| {
            let mut codes: Vec<String> = map.keys().cloned().collect();
            codes.sort();
            codes
        })
        .unwrap_or_default();

    render(ReceiptTemplate {
        outlet_name: settings.outlet_name(),
        cashier: sale.cashier.clone(),
        lines_display: sale
            .lines
            .iter()
            .map(|l| LineDisplay {
                name: l.name.clone(),
                qty: l.qty,
                unit: settings.money(l.unit_price),
                amount: settings.money(l.amount),
            })
            .collect(),
        subtotal_display: settings.money(sale.subtotal),
        discount_display: settings.money(sale.discount),
        total_display: settings.money(sale.total),
        received_display: sale.received.map(|v| settings.money(v)).unwrap_or_default(),
        change_display: sale.change.map(|v| settings.money(v)).unwrap_or_default(),
        is_cash: sale.received.is_some(),
        paper_preview: crate::print::preview(&settings, &sale),
        paper_cols: settings.paper_cols(),
        low_stock,
        flash: take_flash(&session).await,
        sale,
    })
}
