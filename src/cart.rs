//! The till cart. Held in the session as (item, qty) only — prices are re-read from the
//! database when the sale is written, so a stale cart can never change what is charged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CartLine {
    pub item_id: i64,
    pub qty: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cart {
    pub lines: Vec<CartLine>,
}

impl Cart {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Adds one unit, or bumps the quantity when the item is already in the cart.
    pub fn add(&mut self, item_id: i64, qty: i64) {
        if let Some(line) = self.lines.iter_mut().find(|l| l.item_id == item_id) {
            line.qty += qty;
        } else {
            self.lines.push(CartLine { item_id, qty });
        }
    }

    /// Sets an absolute quantity. Zero removes the line — that is the minus button.
    pub fn set_qty(&mut self, item_id: i64, qty: i64) {
        if let Some(line) = self.lines.iter_mut().find(|l| l.item_id == item_id) {
            line.qty = qty;
        }
        self.lines.retain(|l| l.qty > 0);
    }

    pub fn remove(&mut self, item_id: i64) {
        self.lines.retain(|l| l.item_id != item_id);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn total_qty(&self) -> i64 {
        self.lines.iter().map(|l| l.qty).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_the_same_item_twice_bumps_the_quantity() {
        let mut cart = Cart::default();
        cart.add(7, 1);
        cart.add(7, 1);
        assert_eq!(cart.lines.len(), 1);
        assert_eq!(cart.lines[0].qty, 2);
    }

    #[test]
    fn dropping_to_zero_removes_the_line() {
        let mut cart = Cart::default();
        cart.add(7, 2);
        cart.set_qty(7, 1);
        assert_eq!(cart.total_qty(), 1);
        cart.set_qty(7, 0);
        assert!(cart.is_empty());
    }
}
