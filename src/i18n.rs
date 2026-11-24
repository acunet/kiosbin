//! Every string a person reads, in the outlet's language.
//!
//! One table, both languages side by side — a translator can diff it, and the compiler refuses
//! to build when a string exists in one language and not the other (the macro generates the
//! struct from the table, so a field cannot exist without both columns). Adding a language is
//! one more column here; adding a string is one row, and nothing else in the codebase knows
//! which languages exist.
//!
//! Deliberately not a runtime lookup table: a missing key would surface as a blank button in
//! front of a paying customer, and a typo here is a compile error.

/// The language an outlet runs in. The list is closed on purpose — a half-translated screen is
/// worse than an untranslated one, so a new language is a deliberate change, not a config value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Id,
    En,
}

macro_rules! catalog {
    ( $( $field:ident : $id:literal | $en:literal ),+ $(,)? ) => {
        #[derive(Debug, Clone, Copy)]
        pub struct T {
            pub lang: Lang,
            $( pub $field: &'static str, )+
        }

        impl T {
            pub const fn id() -> Self { Self { lang: Lang::Id, $( $field: $id, )+ } }
            pub const fn en() -> Self { Self { lang: Lang::En, $( $field: $en, )+ } }

            /// Every string with its field name — for the completeness test.
            pub fn all(&self) -> Vec<(&'static str, &'static str)> {
                vec![ $( (stringify!($field), self.$field), )+ ]
            }
        }
    };
}

catalog! {
    // ---- shared chrome and labels ------------------------------------------------
    cashier:               "Kasir" | "Cashier",
    sign_in:               "Masuk" | "Sign in",
    sign_out:              "Keluar" | "Sign out",
    cancel:                "Batal" | "Cancel",
    pay:                   "Bayar" | "Pay",
    save_and_print:        "Simpan & cetak nota" | "Save & print receipt",
    back_to_till:          "Kembali ke kasir" | "Back to the till",
    open_shift:            "Buka shift" | "Open shift",
    close_shift:           "Tutup shift" | "Close shift",
    receipt:               "Nota" | "Receipt",
    date:                  "Tanggal" | "Date",
    sub_total:             "Subtotal" | "Subtotal",
    discount:              "Diskon" | "Discount",
    total:                 "Total" | "Total",
    change:                "Kembalian" | "Change",
    cash_received:         "Tunai diterima" | "Cash received",
    method:                "Metode" | "Method",
    expected:              "Seharusnya" | "Expected",
    counted:               "Hitung fisik" | "Counted",
    difference:            "Selisih" | "Difference",
    cash_in_drawer:        "uang di laci" | "cash in the drawer",
    after_saving:          "setelah disimpan" | "after saving",
    required_field:        "wajib diisi" | "required",
    thank_you:             "Terima kasih" | "Thank you",

    // ---- login -------------------------------------------------------------------
    login_hint:            "Masukkan PIN kasir untuk mulai" | "Enter the cashier PIN to start",
    pin:                   "PIN" | "PIN",

    // ---- the till ----------------------------------------------------------------
    cart:                  "Keranjang" | "Cart",
    clear:                 "Kosongkan" | "Clear",
    cart_empty:            "Keranjang masih kosong" | "The cart is empty",
    cart_empty_hint:       "Ketuk menu di kiri untuk mulai." | "Tap a tile on the left to start.",
    hint_add_first:        "tambah hasil pertama" | "add the first match",
    hint_pay:              "bayar" | "pay",
    hint_cancel:           "batal" | "cancel",
    out_of_stock:          "bahan habis" | "out of stock",
    no_search_results:     "Tidak ada item yang cocok dengan pencarian itu." | "Nothing matches that search.",
    next_receipt:          "Nota berikutnya" | "Next receipt",
    shift_active:          "Shift aktif" | "Shift open",
    today:                 "Hari ini" | "Today",
    receipts_sold:         "nota" | "receipts",
    low_stock:             "Bahan berikut sudah nol atau minus:" | "These ingredients are at or below zero:",
    search_placeholder:    "Cari kode atau nama item…" | "Search code or item name…",

    // ---- payment -----------------------------------------------------------------
    payment:               "Pembayaran" | "Payment",
    amount_due:            "Total tagihan" | "Amount due",
    payment_method:        "Metode bayar" | "Payment method",
    cash_tendered:         "Uang diterima (tunai)" | "Cash handed over (cash)",
    invoice_discount:      "Diskon nota (opsional)" | "Receipt discount (optional)",
    leave_payment:         "Keluar dari pembayaran" | "Leave the payment screen",
    receipt_lines:         "Rincian nota" | "Receipt lines",
    items_sold:            "item terjual" | "items sold",

    // ---- the receipt screen ------------------------------------------------------
    new_sale:              "Nota baru" | "New sale",
    about_to_print:        "Yang akan dicetak" | "About to print",

    // ---- opening a shift ---------------------------------------------------------
    open_shift_hint:       "hitung uang tunai yang ada di laci sekarang" | "count the cash in the drawer now",
    opening_float:         "Modal awal di laci" | "Opening cash in the drawer",
    quick_empty:           "Kosong (0)" | "Empty (0)",
    start_selling:         "Mulai jualan" | "Start selling",
    opening_float_note:    "Angka ini dipakai menghitung selisih kas waktu tutup shift. Isi apa adanya — bukan angka yang “seharusnya”."
                         | "This figure is used to work out the cash difference at closing. Enter what is really there — not what should be.",

    // ---- closing a shift ---------------------------------------------------------
    shift_opened:          "shift dibuka" | "shift opened",
    close_shift_now:       "Tutup shift sekarang" | "Close shift now",
    count_note:            "Ketik uang yang benar-benar ada. Kotak kosong akan ditolak — itu disengaja: selisih kas inilah yang dipakai untuk kontrol. Jangan mengisi 0 kalau laci tidak kosong."
                         | "Type the money that is really there. An empty box is refused on purpose: this cash difference is the point of the count. Do not enter 0 if the drawer is not empty.",
    shift_closed:          "Shift ditutup" | "Shift closed",
    done_lock_screen:      "Selesai — kunci layar" | "Done — lock the screen",

    // ---- hardware and stock warnings the topbar shows -----------------------------
    printer_ready:         "Printer siap" | "Printer ready",
    printer_missing:       "Printer belum diset" | "No printer set",
    printer_prints_to:     "Struk dicetak ke" | "Receipts print to",
    printer_none:          "Belum ada printer dikonfigurasi" | "No printer configured yet",
    low_stock_fix:         "Koreksi lewat opname sebelum jualan ramai." | "Put it right with a stock count before the rush.",
    units:                 "item" | "items",
    cols:                  "kolom" | "cols",
    price_missing:         "harga belum diisi" | "no price set",
    sold_out:              "Habis" | "Out",
    cash_goes_to_drawer:   "uang tunai di laci" | "cash into the drawer",
    check_transfer:        "cek mutasi setelahnya" | "check the transfer afterwards",
    difference_explained:  "Selisih 0 = uang fisik cocok dengan catatan. Selisih minus = uang di laci kurang dari catatan."
                         | "A difference of 0 means the cash matches the record. A negative difference means the drawer is short.",
    low_stock_after:       "Bahan berikut sudah nol atau minus setelah nota ini:" | "These ingredients are at or below zero after this receipt:",
    print_not_wired:       "Cetak ke printer thermal belum aktif di langkah ini — teks di atas sudah layout final struk" | "Printing to the thermal printer is not wired up at this step — the text above is the final layout for a",
    hours_short:           "j" | "h",

    // ---- things the cashier is told to do differently ----------------------------
    cart_is_empty:         "keranjang masih kosong" | "the cart is empty",
    cash_short:            "Uang diterima kurang dari total" | "Cash handed over is less than the total",
    sale_not_found:        "nota tidak ditemukan" | "receipt not found",
    count_required:        "jumlah fisik belum diisi untuk" | "no counted amount entered for",
}

impl T {
    /// Outlets name a language in their settings. Unknown or missing means English: the product's
    /// own default (the tenant that happened to come first is not a sensible global default).
    pub fn of(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "id" | "id-id" | "in" | "ind" | "indonesian" => Self::id(),
            _ => Self::en(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_string_is_empty_in_either_language() {
        for (key, value) in T::id().all() {
            assert!(!value.trim().is_empty(), "id.{key} is empty");
        }
        for (key, value) in T::en().all() {
            assert!(!value.trim().is_empty(), "en.{key} is empty");
        }
    }

    /// The strings a shop actually sees on the till. If one of these is the same in both
    /// languages it was never translated — a blank label is bad, an untranslated one is worse.
    #[test]
    fn the_core_screen_is_really_translated() {
        let (id, en) = (T::id(), T::en());
        for (key, id_text) in [
            ("cart", id.cart),
            ("pay", id.pay),
            ("cashier", id.cashier),
            ("close_shift", id.close_shift),
            ("change", id.change),
            ("receipt", id.receipt),
        ] {
            let en_text = en.all().into_iter().find(|(k, _)| *k == key).unwrap().1;
            assert!(
                !en_text.eq_ignore_ascii_case(id_text),
                "en.{key} ({en_text}) was never translated from id.{key} ({id_text})",
            );
        }
    }

    #[test]
    fn unknown_language_codes_fall_back_to_the_product_default() {
        assert_eq!(T::of("").lang, Lang::En);
        assert_eq!(T::of("de").lang, Lang::En);
        assert_eq!(T::of("id").lang, Lang::Id);
        assert_eq!(T::of("ID-ID").lang, Lang::Id);
    }
}
