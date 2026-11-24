# ASSESSMENT — RustPOS (mengganti ERPNext untuk POS + stock)

> **Tanggal:** 2026-11-24 · **Status:** assessment (belum ada kode) · **Target:** repo baru `RustPOS`
> **Yang dinilai:** `docs/rust-pos/AGENTS.warkop-pos.md` + `docs/rust-pos/warkop_seed.json`,
> dibandingkan ulang dengan kode hidup di `pos_next/`, `warkop3dara_app/`, dan catatan prod
> (`docs/CHANGELOG.md`, `docs/DEVELOPMENT_STATUS.md`).
>
> **Cara baca:** §4 = yang harus **didesain baru**, §7 = koreksi atas draft lama, §10 = keputusan
> yang gua tidak boleh tebak sendiri.

---

## 1. Kesimpulan singkat

1. **Scope "cuma stock + POS" itu benar dan cukup.** Yang bikin berat bukan POS-nya, tapi ERPNext
   penuh (accounting, payroll, MRP) yang dipakai untuk 3–11 transaksi/hari. Buang accounting,
   sisakan **satu ledger stok** + POS transaksi.
2. **Rust bukan pekerjaan mahal di sini.** Logika stok yang harus dipindah (resep → potong bahan)
   hanya ±300 baris Python (`recipe_deduction.py` 271 + `uom_utils.py`). Yang mahal justru 3 hal
   yang **belum ada di draft lama**:
   - **Desk hilang** → siapa yang nambah menu, ubah harga, edit resep? (sekarang lewat Desk ERPNext)
   - **Opening stock** — baseline stok sekarang rusak (17 baris `tabBin` negatif, 14 bahan `BB-*` ≤ 0),
     harus ditetapkan sekali sebelum cutover, bukan diimpor apa adanya.
   - **Voucher hotspot MikroTik** — nyata dipakai (142 terbit), dipicu dari invoice POS, dan
     **tidak tercantum sama sekali** di scope/non-goals draft lama.
3. **Perkiraan paritas v1:** POS + stok bahan + shift + struk + laporan + voucher = realistis dalam
   9–11 langkah berurutan (§9). Accounting, wallet, promo, credit, meja, batch/serial: dibuang.

---

## 2. Kondisi sistem sekarang (angka, dengan bukti)

| Hal | Angka | Bukti |
|---|---|---|
| POS Python | 22.703 LOC / 128 file | `pos_next/pos_next/` |
| POS frontend | 55 komponen Vue, ±60.000 LOC `.vue`+`.js` | `pos_next/POS/src/` |
| Endpoint API POS | 113 `@frappe.whitelist` | `pos_next/pos_next/api/*.py` |
| Engine checkout | `invoices.py` 3.325 LOC, `items.py` 2.457 LOC | idem |
| DocType POS | 25 | `pos_next/pos_next/pos_next/doctype/` |
| Report POS | 5 (terbesar `sales_vs_shifts_report.py` 1.239 LOC) | `pos_next/.../report/` |
| Layer integrasi Warkop | 6.446 LOC / 32 file | `warkop3dara_app/` |
| Engine potong stok | 271 LOC + 169 LOC tes | `warkop3dara_app/.../inventory/recipe_deduction.py` |
| Menu & bahan (snapshot read-only 2026-11-21) | 39 item (6 grup) · 35 resep / 70 baris resep · 44 bahan · 2 metode bayar | `docs/rust-pos/warkop_seed.json` |
| UOM bahan di resep | `Gram`, `Ml`, `Pcs` (item menu: `Portion`, `Unit`) | idem |
| Transaksi prod Sep–20 Nov 2026 | 584 nota · Rp 15.361.000 · 3–11 nota/hari · rata-rata Rp 26.303 | `docs/rust-pos/AGENTS.warkop-pos.md` §0 |
| Metode bayar dipakai | Cash + QRIS statis saja | idem |
| Kas keluar | **0** dari 97 shift | `docs/CHANGELOG.md` (keputusan owner 2026-11-21) |
| Voucher hotspot MikroTik | 142 Provisioned, 5 Failed | `AGENTS.warkop-pos.md` §0 |
| Printer | auto-print **mati sejak 11 Okt** (NAT/port 9100 tidak terjangkau, gagal tanpa error) | `docs/CHANGELOG.md` 2026-11-21 |
| Stok | 17 baris `tabBin` negatif, 14 bahan `BB-*` ≤ 0 | idem |

**Tidak bisa diverifikasi dari PC ini:** semua angka prod di atas berasal dari pembacaan read-only
2026-11-21 oleh sesi lain. Belum ada akses DB prod dari sini (`docs/PRODUCTION_READ_ONLY_CHECK.md`
masih berstatus desain: belum ada akses production yang diberikan). Angka di dokumen ini = snapshot
`warkop_seed.json`, bukan query baru.

---

## 3. Yang benar-benar "stock app" bagi POS ini (bukan seluruh modul Stock ERPNext)

Ditelusuri dari kode: yang dipakai POS dari sisi stok cuma ini, dan semuanya gampang diganti.

| Kebutuhan hari ini | Dipakai di | Rencana RustPOS |
|---|---|---|
| `Bin.actual_qty` per warehouse (tampilkan stok di grid/dialog) | `items.py:45-54`, `:279-280`, `:431-439` | ✅ ledger turunan (SUM) — jangan simpan saldo |
| Warehouse tunggal `Stores - W3D` | `AGENTS.warkop-pos.md` §12 | ✅ jadi konstanta `settings.outlet_warehouse` |
| UOM + `UOM Conversion Factor` (resep Gram/Ml/Pcs → stok) | `warkop3dara_app/.../uom_utils.py`, 303 ref `uom` | ✅ **tabel `uom_conversions`** (lihat §4.3) |
| Resep = BOM versi sendiri (`Warkop Recipe` + child) | `warkop3dara_app/.../doctype/warkop_recipe*` | ✅ jadi tabel `recipes` (sudah ada di draft §3) |
| Potong bahan otomatis saat nota submit (Material Issue) | `recipe_deduction.py:211-254` | ✅ port langsung, tapi **tanpa dokumen perantara** (tulis `stock_moves` di transaksi DB nota) |
| Batal/void = reversal | `recipe_deduction.py:257-271` | ✅ sesuai draft §6.6 |
| Waste (Warkop Waste Log → Material Issue) | `warkop_waste_log/` | ⚠️ tanya: dipakai atau tidak? (belum ada angka pemakaian di dokumen) |
| Opname / Stock Reconciliation | 1 kali dibutuhkan → lihat §4.2 | ✅ layar opname sederhana (koreksi saldo + alasan) |
| Harga beli / HPP | `allow_zero_valuation_rate=1` (`recipe_deduction.py:170`) → **stok keluar nilai 0, HPP tidak pernah ada** | ⚠️ fase 2 (pembelian) — sesuai draft §4 |
| Menu item sebagai `is_stock_item=1` ("finished good") | `seeds/setup_master_data.py:370` | ❌ **jangan** dilacak (§4.4) |
| Product bundle / `packed_items` | `pos_next/overrides/packed_item.py` + 228 LOC tes regresi | ❌ tidak dipakai warkop |
| Batch/serial | `BatchSerialDialog.vue` 526 LOC, 46 ref `batch_no` | ❌ tidak dipakai warkop |
| `gl_entry` / pajak / cost center | 7 ref, 47 ref `cost_center` | ❌ dibuang (tanpa accounting) |

Kesimpulan: **"stock app" untuk kasus ini = satu tabel ledger + resep + UOM + opname.** Tidak ada
FIFO/valuation layer, tidak ada multi-warehouse, tidak ada Bundle.

---

## 4. Yang harus DIDESAIN BARU (ini isi jawaban "apa yang bakal kita design")

### 4.1 Master data tanpa Desk ← **gap terbesar di draft lama**

Sekarang owner nambah menu / ubah harga / edit resep lewat **Desk ERPNext**
(`docs/POS_SETUP_GUIDE.md` §5–6: Item, Harga, Recipe). Kalau ERPNext dimatikan, jalur itu hilang.
Draft lama hanya punya `cargo run -- seed` (sekali di awal) → **tidak ada cara ganti harga setelah itu.**

Pilihan (urut dari paling kecil):
1. **CLI + CSV import** (`cargo run -- import items.csv`, idempoten) — 1 layar HTML read-only untuk
   verifikasi. Paling kecil, cukup untuk perubahan menu 1–2 minggu sekali.
2. Halaman owner "Menu & Harga" (form sederhana: nama, grup, harga, aktif) + editor resep.
3. Full CRUD + audit trail.

**Rekomendasi: (1) di v1, (2) di v1.1.** Tanpa salah satu dari ini, v1 = POS yang menunya beku.

### 4.2 Opening stock / baseline stok

Stok prod sekarang **sudah salah** (17 Bin negatif). Jadi impor saldo apa adanya = memindahkan
kerusakan. Yang didesain:
- layar **Opname Awal** (daftar 44 bahan, isi saldo fisik nyata, alasan `opening`) →
  satu batch `stock_moves` bertanggal cutover;
- setelah itu opname biasa (`reason='opname'`, koreksi per bahan + catatan);
- kebijakan stok negatif: **blokir jual kalau bahan ≤ 0?** → keputusan owner (§10 no.4). Draft lama
  tidak menyebut kebijakan ini sama sekali, padahal hari ini stoknya sudah minus.

### 4.3 UOM: satu tabel konversi, jangan string bebas

Draft §3 menyimpan `uom TEXT` saja. Itu cukup untuk resep (Gram/Ml/Pcs) tapi **tidak cukup** begitu
fase 2 (pembelian Kg/Liter/Box → HPP) jalan: butuh tahu 1 Kg = 1000 Gram. Desain:
`uom_conversions(from_uom, to_uom, factor)` + aturan: ledger **selalu** dalam base UOM bahan
(`Gram`, `Ml`, `Pcs`), konversi hanya di batas input/output. Samakan dengan tabel UOM ERPNext
(`docs/SYSTEM_ARCHITECTURE_BASELINE.md` §5.1) supaya seed/import tidak bikin satuan baru.

### 4.4 Bahan saja, atau bahan + menu?

Draft §3 punya `items.kind = menu|addon|raw|service` tapi tidak menjawab: **apakah stok menu
dilacak?** Hari ini ya (`is_stock_item=1` untuk menu) dan hasilnya tidak ada yang memakai angka itu
selain badge di UI (dan ikut menghasilkan Bin negatif).
**Rekomendasi: hanya `raw`/`addon` yang punya ledger.** `menu` + `service` (mis. `MN-HOTSPOT-2JAM`)
tidak. Konsekuensi: badge stok di grid menu hilang (atau menampilkan "bahan terbatas" dari resep).

### 4.5 Voucher hotspot MikroTik — di v1 atau tidak?

Fakta: item `MN-HOTSPOT-2JAM` (grup `Services`, harga 0) dijual di POS → `on_submit` memanggil
`voucher_hooks.issue_voucher_on_submit` → `hotspot/service.py` menerbitkan voucher (mock default,
REST RouterOS opt-in) → kode voucher dicetak di blok **VOUCHER HOTSPOT** struk
(`pos_next_receipt.json`, jinja `get_receipt_vouchers`). Konfigurasi per item lewat custom field:
`hotspot_enabled`, `mikrotik_profile`, `hotspot_duration_minutes`, `hotspot_validity_minutes`
(`hotspot/lines.py:109-152`), kredensial router di Single DocType `Warkop Hotspot Settings`.

Port ke Rust ≈ 1 tabel `vouchers` + 1 panggilan REST + 1 blok struk (~150–200 LOC). **Murah**, tapi
harus diputuskan, kalau tidak, produk voucher mati saat ERPNext dimatikan (§10 no.1).

### 4.6 Nomor nota & laporan paritas

`W3D-YYYYMMDD-NNN` (draft §3) menggantikan `ACC-SINV-2026-xxxxx`. Perlu: (a) urutan aman-konkuren
(dalam transaksi DB yang sama, sesuai draft), (b) kolom `legacy_ref` supaya nota lama masih bisa
dicari di ERPNext selama masa paralel, (c) tabel `legacy_totals` (draft §8) untuk laporan lama.

### 4.7 Satu sumber layout struk

Hari ini layout struk ada **dua kali**: ESC/POS byte builder (`pos_next/api/printer.py` 630 LOC +
`warkop3dara_app/print_format/escpos.py` 317 LOC) dan Jinja print format (`pos_next_receipt.json`).
Keduanya sudah pernah beda isi (blok Expected/Diff dihapus di salah satu jalur saja). Di RustPOS:
**satu fungsi render** yang bisa mengeluarkan byte ESC/POS dan HTML (reprint/layar) dari data yang
sama. Ini menghapus kelas bug yang sudah dibayar mahal (`AGENTS.warkop-pos.md` §11.3).

---

## 5. Yang DIPORT apa adanya (sudah terbukti di prod)

| Modul | Sumber | Catatan |
|---|---|---|
| Potong stok dari resep (modifier-aware, idempoten, reversal) | `inventory/recipe_deduction.py` | idempotensi via stamp → di Rust: unique index `(sale_id, item_id)` atau simpan `deducted_at` di nota |
| Konversi UOM | `inventory/uom_utils.py` | + tabel konversi (§4.3) |
| Struk thermal + EOD | `print_format/escpos.py`, `pos_next/api/printer.py` | 32/48 kolom, satu fungsi render (§4.7) |
| Tutup shift + hitung kas (Expected = opening + cash; QRIS tidak masuk expected) | `pos_closing_shift.py` 707 LOC | sudah diringkas di draft §6.1–6.3 — pertahankan aturannya |
| QRIS statis (gambar QR, kasir tap "Sudah Bayar") | `warkop/QrisPaymentDialog.vue` | tanpa gateway |
| Laporan owner: harian, menu terjual, kas per shift, stok + CSV | 5 report Frappe | 4 laporan minimal (draft §4) |
| Barcode/scan | `services/barcode.py` 299 LOC | kalau pencetakan barcode dipakai; kalau tidak, skip |

---

## 6. Yang DIBUANG (dan tidak usah dibikin "buat nanti")

Accounting/GL + pajak + multi-currency + cost center · wallet & loyalty (`wallet.py` 478 + DocType
618 LOC) · promo/offer/coupon/pricing rule (969+644 LOC) · credit sales (712 LOC) · partial
payments (975 LOC) · bank deposit & referral · meja/KDS · batch/serial · product bundle/packed items
· offline PWA/service worker (`offline.worker.js` 1.937 LOC) · QZ Tray/jembatan print desktop ·
localization (ar/id/pt-br) · kas keluar (keputusan owner: 0 pemakaian) · multi-outlet · payroll ·
manufacturing.

---

## 7. Koreksi atas `docs/rust-pos/AGENTS.warkop-pos.md` (hasil "cek assessment ini")

Yang sudah bagus dan jangan diubah: stack (§1), aturan uang/stok (§6), daftar jebakan (§11), urutan
eksekusi (§0), keputusan yang sudah dikunci (§12).

Yang perlu diperbaiki:

| # | Temuan | Dampak kalau dibiarkan |
|---|---|---|
| 1 | **Voucher hotspot tidak ada** di scope §4 maupun non-goals §0 | produk `MN-HOTSPOT-2JAM` + 142 voucher mati diam-diam saat ERPNext dimatikan |
| 2 | **Tidak ada jalur ubah master data** setelah seed | harga/menu/resep beku; ini regresi harian, bukan fitur "nanti" |
| 3 | **Opening stock tidak disebut** (padahal 17 Bin negatif, 14 bahan ≤ 0) | ledger baru mulai dari warisan yang sudah rusak |
| 4 | §8 bilang "39 item" sementara `docs/CHANGELOG.md` menulis "38 item" | kecil, tapi ini jenis salah-hitungan yang bikin ribet saat rekonsiliasi → **angka benar dari seed: 39 item / 35 resep / 70 baris resep / 44 bahan** |
| 5 | Skema §3: `uom TEXT`, tanpa tabel konversi | fase 2 (pembelian/HPP) mentok |
| 6 | Skema §3: tidak ada kolom custom hotspot per item (`hotspot_enabled`, `mikrotik_profile`, `hotspot_duration_minutes`, `hotspot_validity_minutes`) | voucher tidak bisa dikonfigurasi |
| 7 | Status "menu item = stock item" belum diputuskan (§4.4 dok ini) | badge stok menu jadi angka palsu/negatif |
| 8 | Kebijakan stok negatif belum ada (blokir vs izinkan + catat) | kasir bisa jual bahan yang sudah minus tanpa keputusan sadar |
| 9 | §9 daftar tes belum mencakup: idempotensi potong stok (replay/double-submit), opname, penomoran nota 2 kasir bersamaan, gagal cetak → log + badge | tiga di antaranya sudah pernah jadi bug nyata (§11.1, §11.3) |
| 10 | §7 menyebut versi crate "diverifikasi 2026-11-21" | cukup, tapi verifikasi ulang saat `cargo new` (`axum 0.8`, `tokio`, `sqlx 0.9`, `askama 0.16`, `tower-sessions 0.15`) |
| 11 | Rencana paralel "1 minggu" (§8) tanpa mekanisme | tambah: skrip rekonsiliasi per-shift (total ERPNext vs Rust) + kriteria numerik "cocok" sebelum ERPNext di-disable |
| 12 | Waste Log tidak diputuskan | kalau dipakai, port; kalau tidak, buang dari desain (jangan sisakan tabel kosong) |

---

## 8. Risiko

| Risiko | Kenapa nyata di sini | Mitigasi |
|---|---|---|
| Ledger baru mulai dari stok negatif | 17 Bin negatif hari ini | opname awal (§4.2) sebelum cutover, tanggal cutover dicatat |
| Receipt hilang = bukti jual hilang | auto-print mati sejak 11 Okt tanpa error | tombol Tes Printer + log kegagalan + cetak ulang dari riwayat nota (sudah di draft §11.1) |
| ERPNext dimatikan terlalu cepat | laporan owner & history ada di sana | paralel + rekonsiliasi angka per shift; ERPNext di-disable, DB **jangan** dihapus |
| Voucher putus di tengah | bergantung hook invoice + REST router | putuskan §4.5; kalau ditunda, dokumentasikan "voucher tetap di ERPNext sampai fase 2" |
| Nomor nota bentrok | 2 tablet/kasir, SQLite | nomor dibuat di transaksi DB yang sama (draft §3) + tes konkuren |
| Tablet butuh WiFi ke PC server | arsitektur lokal (bukan cloud) | keputusan owner: terima, atau tablet autofallback? (draft §12b, masih terbuka) |
| Master data beku | Desk hilang tanpa pengganti | §4.1 jalan paling kecil di v1 |
| HPP/laba tidak ada | `allow_zero_valuation_rate=1` → stok keluar nilai 0 | laporan laba menampilkan "HPP belum diisi", bukan laba 100% (draft §6.7) |

---

## 9. Urutan eksekusi (revisi dari draft §0)

1. `cargo new` + `Cargo.toml` + `migrations/0001_init.sql` (skema §3 draft **+** §4.3/4.6/4.7 dok ini) → `cargo run -- migrate`.
2. Seed dari `warkop_seed.json` (39 item / 35 resep / 44 bahan), idempoten + gagal keras kalau bahan resep tidak ada.
3. **Opname awal** (44 bahan, isi saldo fisik) → `stock_moves` alasan `opening`.
4. Auth PIN kasir + halaman Buka Shift.
5. Grid menu + cart (HTMX partial), cek visual.
6. Bayar cash + QRIS + simpan nota + potong stok **dalam satu transaksi DB**; tes §9 draft hijau.
7. Struk thermal + Tes Printer → **cetak kertas nyata** sebelum lanjut.
8. Tutup shift + EOD.
9. Laporan + CSV (+ `legacy_totals` untuk omzet lama).
10. Voucher hotspot (§4.5) — kalau owner setuju masuk v1.
11. Master data minimal (§4.1) → systemd + backup harian → paralel + rekonsiliasi 1 minggu.
12. Baru fase 2: pembelian + UOM Kg/Liter → HPP & laba kotor.

Setiap langkah = satu commit. Item draft §0 no.3–9 tetap urutannya; yang berubah: tambah opname
sebelum jualan, voucher & master data sebelum paralel.

---

## 10. Keputusan yang harus dijawab owner (gua tidak menebak)

1. **Voucher hotspot MikroTik:** di-port ke RustPOS di v1, atau tetap di ERPNext (ERPNext tidak boleh
   dimatikan total) sementara?
2. **Master data:** cukup CLI/CSV import di v1, atau sekalian halaman owner "Menu & Harga"?
3. **Stok menu item:** ikut dilacak (seperti sekarang) atau bahan saja?
4. **Stok negatif:** blokir penjualan kalau bahan ≤ 0, atau izinkan + tandai?
5. **Waste Log (catatan bahan terbuang):** dipakai atau tidak?
6. **Lebar kertas printer:** 58 mm (32 kolom) atau 80 mm (48 kolom)? (printer belum bisa dijangkau
   sejak 11 Okt, jadi ini sekalian diverifikasi saat langkah 7)
7. **Lokasi repo `RustPOS`:** `C:\Users\mansu\dev\RustPOS` (repo git baru, sejajar
   `warkop3dara.id`) atau di dalam repo ini?
8. **Tablet kedua / kasir kedua bersamaan** dan **WiFi mati = POS mati** (draft §12b, masih terbuka).

---

## 11. Rencana isi folder `RustPOS`

```
RustPOS/
├── AGENTS.md                 # dari docs/rust-pos/AGENTS.warkop-pos.md (revisi sesuai §7 dok ini)
├── Cargo.toml · migrations/ · seed/warkop_seed.json
├── src/
│   ├── main.rs · db.rs · money.rs
│   ├── domain/  catalog · sale · stock · shift · voucher   (voucher = baru)
│   ├── print/   escpos · receipt                            (satu renderer, §4.7)
│   ├── web/     kasir · shift · laporan · master data (baru) · settings
│   ├── templates/ (Askama)
│   └── import/  seed.rs · items_csv.rs · legacy_totals.rs (baru)
├── tests/       uang · stok · shift · resep idempoten · nomor nota konkuren · voucher
└── ops/         warkop-pos.service · backup.sh · reconcile_vs_erpnext.sh (baru)
```

Dokumen ini **tidak** menggantikan `AGENTS.warkop-pos.md` — ia daftar perubahan yang harus masuk ke
sana sebelum repo `RustPOS` dibuat. Setelah owner menjawab §10, langkah berikutnya: patch
`AGENTS.warkop-pos.md` (12 temuan §7) lalu scaffolding `RustPOS` langkah 1.

**Assessment dianggap selesai bila:** (a) §10 terjawab, (b) AGENTS.md RustPOS sudah memuat
master-data, opening stock, kebijakan stok negatif, dan keputusan voucher, (c) skema `0001_init.sql`
memuat tabel `uom_conversions` + kolom hotspot + `legacy_ref`.
