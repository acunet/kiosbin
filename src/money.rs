//! Rupiah maths. Money is always a whole number of rupiah — never f64.

/// `26303` -> `"Rp 26.303"`
pub fn format_rupiah(amount: i64) -> String {
    let negative = amount < 0;
    let digits = amount.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);

    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(ch);
    }

    if negative {
        format!("-Rp {grouped}")
    } else {
        format!("Rp {grouped}")
    }
}

/// Parses what a cashier actually types: `"26.000"`, `"Rp 26000"`, `"1,250,000"`.
///
/// Rupiah has no cents, so every non-digit is treated as a thousands separator.
/// Returns `None` when there is no digit at all — never silently returns 0.
pub fn parse_rupiah(input: &str) -> Option<i64> {
    let digits: String = input.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_with_thousand_dots() {
        assert_eq!(format_rupiah(0), "Rp 0");
        assert_eq!(format_rupiah(26_303), "Rp 26.303");
        assert_eq!(format_rupiah(1_250_000), "Rp 1.250.000");
        assert_eq!(format_rupiah(-45_000), "-Rp 45.000");
    }

    #[test]
    fn parses_what_the_cashier_types() {
        assert_eq!(parse_rupiah("26.000"), Some(26_000));
        assert_eq!(parse_rupiah("Rp 1.250.000"), Some(1_250_000));
        assert_eq!(parse_rupiah("50000"), Some(50_000));
        // A typed zero is a real count, not "empty".
        assert_eq!(parse_rupiah("0"), Some(0));
    }

    #[test]
    fn empty_input_is_not_zero() {
        assert_eq!(parse_rupiah(""), None);
        assert_eq!(parse_rupiah("Rp"), None);
    }
}
