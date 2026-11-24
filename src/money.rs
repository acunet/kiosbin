//! Money handling. Amounts are integers in the currency's minor unit — never floats.

/// Currency display rules. `decimals = 0` is rupiah (1 minor unit = Rp 1);
/// `decimals = 2` is a cents-style currency.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Currency {
    pub symbol: String,
    pub decimals: u32,
    pub group_sep: char,
}

impl Currency {
    pub fn rupiah() -> Self {
        Currency {
            symbol: "Rp".to_string(),
            decimals: 0,
            group_sep: '.',
        }
    }

    /// `1_250_000` with rupiah prints `Rp 1.250.000`; with a 2-decimal currency
    /// `125_000_000` prints `$ 1,250,000.00`-style according to `group_sep`.
    pub fn format(&self, minor_units: i64) -> String {
        let negative = minor_units < 0;
        let mut digits = minor_units.unsigned_abs().to_string();
        let decimals = self.decimals as usize;

        if decimals > 0 {
            while digits.len() <= decimals {
                digits.insert(0, '0');
            }
            let split = digits.len() - decimals;
            digits.insert(split, '.');
        }

        let (whole, fraction) = match digits.split_once('.') {
            Some((whole, fraction)) => (whole.to_string(), Some(fraction.to_string())),
            None => (digits, None),
        };

        let mut grouped = String::with_capacity(whole.len() + whole.len() / 3 + 4);
        for (i, ch) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i).is_multiple_of(3) {
                grouped.push(self.group_sep);
            }
            grouped.push(ch);
        }

        let body = match fraction {
            Some(fraction) => format!("{grouped}.{fraction}"),
            None => grouped,
        };
        let sign = if negative { "-" } else { "" };
        format!("{sign}{} {body}", self.symbol)
    }
}

/// Reads what a cashier types: `"26.000"`, `"Rp 26000"`, `"50000"`.
///
/// Every non-digit is treated as a separator, which is exact for zero-decimal currencies
/// (rupiah): the till keypad and the parsing agree.
/// shortcut: a 2-decimal currency typed as `"12.5"` would read as 125 minor units, so a
/// decimal-aware keypad is needed before selling in a cents currency.
pub fn parse(input: &str) -> Option<i64> {
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
    fn formats_rupiah_with_thousand_dots() {
        let idr = Currency::rupiah();
        assert_eq!(idr.format(0), "Rp 0");
        assert_eq!(idr.format(26_303), "Rp 26.303");
        assert_eq!(idr.format(1_250_000), "Rp 1.250.000");
        assert_eq!(idr.format(-45_000), "-Rp 45.000");
    }

    #[test]
    fn formats_a_two_decimal_currency() {
        let usd = Currency {
            symbol: "$".to_string(),
            decimals: 2,
            group_sep: ',',
        };
        assert_eq!(usd.format(0), "$ 0.00");
        assert_eq!(usd.format(1_250), "$ 12.50");
        assert_eq!(usd.format(125_000_000), "$ 1,250,000.00");
    }

    #[test]
    fn parses_what_the_cashier_types() {
        assert_eq!(parse("26.000"), Some(26_000));
        assert_eq!(parse("Rp 1.250.000"), Some(1_250_000));
        assert_eq!(parse("50000"), Some(50_000));
        // A typed zero is a real count, not "empty".
        assert_eq!(parse("0"), Some(0));
    }

    #[test]
    fn empty_input_is_not_zero() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("Rp"), None);
    }
}
