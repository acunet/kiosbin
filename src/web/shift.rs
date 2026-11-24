//! Opening and closing a shift, and the cash reconciliation that goes with it.

use std::collections::HashMap;

use askama::Template;
use axum::extract::State;
use axum::response::Response;
use axum::Form;
use tower_sessions::Session;

use crate::web::{bad_request, redirect, render, set_flash, take_flash, whoami, AppState};

#[derive(Template)]
#[template(path = "shift_open.html")]
struct ShiftOpenTemplate {
    outlet_name: String,
    cashier: String,
    flash: String,
}

#[derive(Clone)]
struct CountView {
    method: String,
    expected_display: String,
    counted_display: String,
    diff_display: String,
    is_cash: bool,
    matched: bool,
}

#[derive(Template)]
#[template(path = "shift_close.html")]
struct ShiftCloseTemplate {
    outlet_name: String,
    cashier: String,
    opened_at: String,
    lines: Vec<CountView>,
    flash: String,
}

#[derive(Template)]
#[template(path = "shift_closed.html")]
struct ShiftClosedTemplate {
    outlet_name: String,
    cashier: String,
    closed_at: String,
    lines: Vec<CountView>,
}

pub async fn open_page(State(state): State<AppState>, session: Session) -> Response {
    let Some(user) = whoami(&session).await else {
        return redirect("/login");
    };
    let settings = match state.settings().await {
        Ok(s) => s,
        Err(e) => return bad_request(e.to_string()),
    };
    match crate::shift::open_for(&state.pool, &user.cashier).await {
        Ok(Some(_)) => redirect("/"),
        Ok(None) => render(ShiftOpenTemplate {
            outlet_name: settings.outlet_name(),
            cashier: user.cashier,
            flash: take_flash(&session).await,
        }),
        Err(e) => bad_request(e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct OpenShiftForm {
    #[serde(default)]
    pub opening_float: String,
}

pub async fn open(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<OpenShiftForm>,
) -> Response {
    let Some(user) = whoami(&session).await else {
        return redirect("/login");
    };
    let opening_float = crate::money::parse(&form.opening_float).unwrap_or(0);
    match crate::shift::open(&state.pool, &user.cashier, opening_float).await {
        Ok(_) => redirect("/"),
        Err(e) => {
            set_flash(&session, e.to_string()).await;
            redirect("/shift/open")
        }
    }
}

pub async fn close_page(State(state): State<AppState>, session: Session) -> Response {
    let Some(user) = whoami(&session).await else {
        return redirect("/login");
    };
    let settings = match state.settings().await {
        Ok(s) => s,
        Err(e) => return bad_request(e.to_string()),
    };
    let shift = match crate::shift::open_for(&state.pool, &user.cashier).await {
        Ok(Some(shift)) => shift,
        Ok(None) => return redirect("/shift/open"),
        Err(e) => return bad_request(e.to_string()),
    };
    let expected = match crate::shift::expected(&state.pool, &shift, &settings).await {
        Ok(rows) => rows,
        Err(e) => return bad_request(e.to_string()),
    };

    render(ShiftCloseTemplate {
        outlet_name: settings.outlet_name(),
        cashier: user.cashier,
        opened_at: shift.opened_at.clone(),
        lines: expected
            .into_iter()
            .map(|row| CountView {
                method: row.method,
                expected_display: settings.money(row.expected),
                counted_display: String::new(),
                diff_display: String::new(),
                is_cash: row.is_cash,
                matched: false,
            })
            .collect(),
        flash: take_flash(&session).await,
    })
}

pub async fn close(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let Some(user) = whoami(&session).await else {
        return redirect("/login");
    };
    let settings = match state.settings().await {
        Ok(s) => s,
        Err(e) => return bad_request(e.to_string()),
    };
    let shift = match crate::shift::open_for(&state.pool, &user.cashier).await {
        Ok(Some(shift)) => shift,
        Ok(None) => return redirect("/shift/open"),
        Err(e) => return bad_request(e.to_string()),
    };

    // Every configured method must be answered. A blank box stays blank — the reconciliation
    // rule is that the cashier types what is physically in the drawer.
    let counts: Vec<(String, Option<i64>)> = settings
        .payment_methods()
        .into_iter()
        .map(|method| {
            let raw = form
                .get(&format!("counted_{method}"))
                .cloned()
                .unwrap_or_default();
            let value = crate::money::parse(&raw);
            (method, value)
        })
        .collect();

    match crate::shift::close(&state.pool, &shift, &settings, &counts).await {
        Ok(lines) => render(ShiftClosedTemplate {
            outlet_name: settings.outlet_name(),
            cashier: user.cashier,
            closed_at: sqlx::query_scalar::<_, String>(crate::db::NOW_SQL)
                .fetch_one(&state.pool)
                .await
                .unwrap_or_default(),
            lines: lines
                .into_iter()
                .map(|line| CountView {
                    method: line.method,
                    expected_display: settings.money(line.expected),
                    counted_display: settings.money(line.counted),
                    diff_display: settings.money(line.diff),
                    is_cash: line.is_cash,
                    matched: line.diff == 0,
                })
                .collect(),
        }),
        Err(e) => {
            set_flash(&session, e.to_string()).await;
            redirect("/shift/close")
        }
    }
}
