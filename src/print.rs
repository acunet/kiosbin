//! Receipt layout — the single source for both the screen preview and (later) the
//! ESC/POS bytes sent to the thermal printer. One layout, two outputs.

use crate::sale::SaleResult;
use crate::settings::Settings;

fn centre(text: &str, cols: usize) -> String {
    let text = text.chars().take(cols).collect::<String>();
    let pad = cols.saturating_sub(text.chars().count()) / 2;
    format!("{}{}", " ".repeat(pad), text)
}

/// Left label, right value, padded to the paper width.
fn row(left: &str, right: &str, cols: usize) -> String {
    let right_len = right.chars().count();
    let room = cols.saturating_sub(right_len + 1);
    let left = if left.chars().count() > room {
        left.chars().take(room).collect::<String>()
    } else {
        left.to_string()
    };
    let pad = cols.saturating_sub(left.chars().count() + right_len);
    format!("{left}{}{right}", " ".repeat(pad))
}

fn rule(cols: usize) -> String {
    "-".repeat(cols)
}

fn wrap(name: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in name.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// The full receipt as monospace lines, exactly `paper_cols` wide.
pub fn lines(settings: &Settings, sale: &SaleResult) -> Vec<String> {
    let cols = settings.paper_cols();
    let mut out = vec![
        centre(&settings.outlet_name().to_uppercase(), cols),
        String::new(),
        row("No", &sale.number, cols),
        row("Tanggal", sale.at.get(..16).unwrap_or(&sale.at), cols),
        row("Kasir", &sale.cashier, cols),
        rule(cols),
    ];

    for line in &sale.lines {
        for (i, chunk) in wrap(&line.name, cols.saturating_sub(4))
            .into_iter()
            .enumerate()
        {
            if i == 0 {
                out.push(chunk);
            } else {
                out.push(format!("  {chunk}"));
            }
        }
        out.push(row(
            &format!("  {} x {}", line.qty, settings.money(line.unit_price)),
            &settings.money(line.amount),
            cols,
        ));
    }

    out.push(rule(cols));
    out.push(row("Subtotal", &settings.money(sale.subtotal), cols));
    if sale.discount > 0 {
        out.push(row(
            "Diskon",
            &format!("-{}", settings.money(sale.discount)),
            cols,
        ));
    }
    out.push(row("TOTAL", &settings.money(sale.total), cols));
    out.push(rule(cols));
    out.push(row(&sale.method, &settings.money(sale.total), cols));
    if let (Some(received), Some(change)) = (sale.received, sale.change) {
        out.push(row("Tunai", &settings.money(received), cols));
        out.push(row("Kembali", &settings.money(change), cols));
    }

    out.push(String::new());
    out.push(centre("Terima kasih", cols));
    out.push(String::new());
    out
}

pub fn preview(settings: &Settings, sale: &SaleResult) -> String {
    lines(settings, sale).join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sale::SaleLineView;

    fn settings() -> Settings {
        let mut map = std::collections::HashMap::new();
        map.insert("outlet_name".to_string(), "Warkop 3Dara".to_string());
        map.insert("paper_cols".to_string(), "32".to_string());
        map.insert("invoice_prefix".to_string(), "W3D".to_string());
        Settings::from_map(map)
    }

    fn sale() -> SaleResult {
        SaleResult {
            id: 1,
            number: "W3D-20261124-001".to_string(),
            at: "2026-11-24 10:15:00".to_string(),
            cashier: "Rina".to_string(),
            lines: vec![SaleLineView {
                item_id: 1,
                name: "Kopi Susu Gula Aren Panas Sekali".to_string(),
                qty: 2,
                unit_price: 18_000,
                amount: 36_000,
            }],
            subtotal: 36_000,
            discount: 0,
            total: 36_000,
            method: "Cash".to_string(),
            received: Some(50_000),
            change: Some(14_000),
            stock_rows: 4,
        }
    }

    #[test]
    fn every_line_fits_the_paper() {
        let text = lines(&settings(), &sale());
        for line in &text {
            assert!(
                line.chars().count() <= 32,
                "line wider than the paper: {line:?}"
            );
        }
    }

    #[test]
    fn a_long_item_name_wraps_instead_of_being_cut_off() {
        let text = lines(&settings(), &sale());
        assert!(text.iter().any(|l| l.contains("Kopi Susu Gula")));
        assert!(text.iter().any(|l| l.contains("Aren Panas")));
    }

    #[test]
    fn cash_shows_the_change() {
        let text = lines(&settings(), &sale()).join("\n");
        assert!(text.contains("Kembali"));
        assert!(text.contains("Rp 14.000"));
        assert!(text.contains("W3D-20261124-001"));
    }
}
