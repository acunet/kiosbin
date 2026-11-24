# AGENTS.md — Warkop 3Dara POS (RustPOS)

> **Cara pakai:** baca sampai habis sebelum menulis kode. Aturan §6 (uang), §7 (stok), dan §10
> (cetak) tidak boleh dilanggar tanpa alasan tertulis di commit message.
> Bahasa: **kode, README, commit message = Inggris. Dokumen ops (file ini, `docs/`) = Indonesia.
> UI kasir & struk = Indonesia** (kasir dan pelanggannya orang Indonesia).

Repo ini adalah pengganti stack Frappe v15 + ERPNext v15 + POSNext untuk **satu warkop, satu
outlet, satu–dua tablet**. ERPNext penuh (accounting, payroll, MRP, 113 endpoint, ±23k LOC Python
+ ±60k LOC frontend) tidak sebanding untuk **3–11 transaksi/hari**.

Repo `warkop3dara.id` (Python/ERPNext) = **acuan data & perilaku**, bukan bagian dari build.
Jangan menambahkan dependensi runtime ke sana.

---

## 0. Status sekarang

| Sudah jalan | Belum |
|---|---|
| `migrate` (skema 13 tabel), `seed` (39 item + 44 bahan + 70 baris resep), `check`, 9 tes hijau | Halaman kasir, bayar, struk, shift, laporan, voucher, master-data UI |

`cargo test` = 9/9 (3 unit uang + 6 integrasi). Semua langkah berikut = satu commit sendiri,
jangan menulis 4 lapis sekaligus lalu debug bersama.

**Urutan yang sudah disepakati** (`docs/ASSESSMENT.md` §9):
migrate → seed → **opname awal** → buka shift/PIN → grid+cart → bayar cash/QRIS + potong stok →
struk + tes printer → tutup shift/EOD → laporan+CSV → voucher hotspot → master data → systemd +
backup → paralel dengan ERPNext 1 minggu → fase 2 (pembelian/HPP).

---

## 1. Stack

| Layer | Pilihan | Alasan |
|---|---|---|
| Bahasa | Rust (edition 2021, stable) | satu binary, tidak ada dependency hell |
| Web | `axum` + `tokio` | router + handler, cukup |
| DB | **SQLite** (`sqlx`, WAL) — satu file | backup = copy file |
| Frontend | **HTMX + Askama** (server-rendered) | nol npm, nol build step, nol stale bundle |
| Sesi | cookie + PIN kasir | tidak perlu email/OAuth |
| Cetak | ESC/POS raw via `TcpStream` ke `settings.printer_host:printer_port` | sudah terbukti di POS lama |
| QRIS | gambar statis milik warung | tanpa payment gateway |

Crate yang sudah dipakai: `sqlx 0.9` (runtime-tokio/sqlite/migrate), `tokio 1`, `serde`, `serde_json`.
Yang akan ditambah di langkah web: `axum 0.8`, `askama`, `tower-sessions` — **jangan tambah
dependensi lain tanpa alasan**; kalau ragu, tulis dulu kenapa di commit message.

**Kenapa HTMX, bukan SPA:** server ada di tempat (PC lokal), jadi "offline" tidak perlu service
worker/IndexedDB. PWA + IndexedDB = sumber bug blank screen/stale bundle di sistem lama.

---

## 2. Layout

```
RustPOS/
├── AGENTS.md                 # file ini
├── Cargo.toml · migrations/ · seed/warkop_seed.json
├── docs/ASSESSMENT.md        # hasil assessment scope (acuan keputusan)
├── src/
│   ├── main.rs               # CLI: migrate | seed | check  (nanti: serve)
│   ├── lib.rs · db.rs        # pool + migrate (WAL, foreign_keys ON, busy_timeout 5s)
│   ├── money.rs              # Rupiah = i64; format & parse
│   └── seed.rs               # impor katalog dari snapshot ERPNext
├── tests/lifecycle.rs        # migrasi, constraint, idempotensi seed
└── ops/                      # (belum) systemd unit, backup.sh, reconcile_vs_erpnext.sh
```

`warkop.db` (dan `-wal`/`-shm`) **jangan pernah** di-commit — sudah ada di `.gitignore`.

---

## 3. Skema & aturan turunan

Skema hidup di `migrations/*.sql` (jangan diedit setelah dipakai di produksi — tambah file baru).
Ringkas: `items` (+ `uom`/`kind`/`hotspot_*`) · `recipes` · `uom_conversions` · `stock_moves` ·
`shifts` · `sales` · `sale_lines` · `payments` · `shift_counts` · `vouchers` · `legacy_totals` ·
`users` · `settings`.

- **Saldo stok = `SUM(stock_moves.qty)`.** Tidak ada kolom `stock`/`actual_qty`. Satu-satunya cara
  saldo jadi salah adalah menulis saldo.
- `kind`: `menu` | `addon` | `raw` | `service`. Hanya `raw` (+ `addon`) yang punya ledger.
  Menu **tidak** dilacak sebagai stok — di sistem lama menu `is_stock_item=1` dan itu sumber
  `tabBin` negatif yang tidak pernah dipakai siapa pun.
- `price = 0` = kasir **wajib** memasukkan harga (mis. `MN-KOPI-07` Max Tea Tarik). Jangan pernah
  menjual Rp 0 diam-diam.
- Satu kasir hanya boleh punya satu shift `open` — dijaga partial unique index, bukan cuma kode.
- Nomor nota: `W3D-YYYYMMDD-NNN`, urut per hari, dibuat **di dalam transaksi DB yang sama** dengan
  insert `sales`. Jangan `MAX()+1` di luar transaksi.
- `sales.legacy_ref` = nomor `ACC-SINV-…` lama, dipakai saat masa paralel.

---

## 4. Perintah kerja

```bash
export PATH="$HOME/.cargo/bin:$PATH"      # git-bash Windows
cargo run -- migrate                      # buat/upgrade warkop.db
cargo run -- seed seed/warkop_seed.json   # impor katalog (idempoten)
cargo run -- check                        # jumlah per kind, resep, saldo ≤ 0, item tanpa harga
cargo test                                # 9 tes
cargo fmt && cargo clippy --all-targets -- -D warnings
```

Output nyata yang diharapkan:

```text
migrate: ok  db=warkop.db  migrations=migrations
seed: ok  items written=83 kept=0  recipe lines=70  uom conversions added=4  settings added=8
seed: ok  items written=0 kept=83  recipe lines=0  uom conversions added=0  settings added=0
items[addon] = 6 · items[menu] = 32 · items[raw] = 44 · items[service] = 1 · recipes = 70
```

`--db PATH` atau env `WARKOP_DB` untuk database lain (dipakai tes).

---

## 5. Master data (Desk ERPNext tidak ada lagi)

- `seed` **tidak pernah menimpa** baris yang sudah ada → harga yang diubah owner tidak akan
  dikembalikan diam-diam oleh re-run.
- `seed --force` = re-read kolom katalog (`name`, `group_name`, `price`, `uom`, `kind`, `active`)
  dan qty resep. Pakai hanya saat katalog di sumber memang berubah.
- Resep yang menunjuk bahan tidak dikenal → **gagal keras, seluruh impor di-rollback**. Jangan
  "lewati saja" (`docs/ASSESSMENT.md` §7 no.6).
- Perubahan menu harian: langkah berikutnya adalah impor CSV / halaman "Menu & Harga".
  Sampai itu ada, ubah harga = `sqlite3`/`--force` — dan itu **belum layak** untuk owner. Lihat §14.

---

## 6. Aturan uang (WAJIB)

1. **Uang selalu `i64` rupiah.** Tidak ada `f64` di jalur uang. Format & parse lewat `money.rs`.
2. Pembulatan ke rupiah penuh **saat hitung**, bukan saat simpan.
3. **Jangan auto-isi `counted` dari `expected`.** Kasir wajib mengetik angka fisik. (Di sistem lama
   16 dari 20 shift mengetik `0` → tampak seperti kas kurang.)
4. `expected` cash = `opening_float + penjualan cash`. **QRIS tidak menambah expected cash.**
   Hitung ulang dari ledger tiap kali (idempoten), jangan disimpan lalu ditambah-tambah.
5. QRIS `counted` = angka settlement/cek mutasi, diisi manual. Jangan menyimpulkan "QRIS kurang"
   dari nominal yang diketik kasir di kolom pembayaran.
6. Kas keluar / belanja laci: **tidak dibangun** (0 pemakaian dari 97 shift). Bukan "pending".
7. Diskon: nominal + persen di level nota. Add-on berbayar = **baris sendiri** (punya resep/stok),
   catatan dapur = teks. Jangan pernah menaikkan harga baris induk untuk menutupi add-on.
8. Void = `status='void'` + `stock_moves` pembalik. Jangan `DELETE`, nomor nota tidak boleh dipakai
   ulang.

---

## 7. Aturan stok (WAJIB)

1. **Opname awal dulu.** Baseline prod sekarang rusak (17 baris `tabBin` negatif, 14 bahan `BB-*`
   ≤ 0). Sebelum transaksi pertama: isi saldo fisik 44 bahan dengan `reason='opening'` dan tanggal
   cutover di `ref`. Jangan mengimpor saldo ERPNext apa adanya.
2. Ledger hanya mencatat **bahan** (`raw`) dan `addon`. Menu & service tidak.
3. Satu penjualan = **satu set** `stock_moves` (satu baris per bahan resep × qty terjual).
   `sales.deducted_at` di-set pada transaksi yang sama → pemicu ulang (retry/replay) tidak dobel.
4. Konversi UOM: `uom_conversions` hanya menyimpan pasangan non-trivial (Kg→Gram 1000,
   Liter→Ml 1000, Box→Pcs 24, Carton→Pack 12). `from = to` = faktor 1. Ledger **selalu** dalam base
   UOM bahan (`Gram`, `Ml`, `Pcs`); konversi hanya di batas input/output.
5. Stok negatif: kebijakan belum diputuskan owner (§14 no.4). Selama belum diputuskan: **izinkan
   tapi tandai** di UI + laporan; jangan diam-diam memblokir penjualan.
6. Opname koreksi: `reason='opname'` + catatan. Waste (`reason='waste'`) baru dipakai kalau owner
   bilang Waste Log dipakai (§14 no.5).
7. HPP: `items.cost = 0` berarti belum diisi. Laporan laba **wajib** menampilkan "HPP belum diisi",
   jangan pernah menampilkan laba 100%.

---

## 8. Alur kasir (definition of v1)

1. Buka Shift: pilih kasir + PIN, isi modal awal.
2. Kasir: grid menu per grup, cari nama/kode, tap → cart kanan. `+ / −`, hapus, add-on, catatan.
3. Bayar: Cash (keypad nominal + kembalian) atau QRIS (gambar QR + tombol "Sudah Bayar").
4. Simpan → struk tercetak (gagal cetak ≠ gagal transaksi; ada tombol cetak ulang) → cart kosong.
5. Tutup Shift: per metode `opening / penjualan / counted / selisih`, cetak EOD.
6. Laporan owner: harian, menu terjual, kas per shift, stok, CSV — bisa dibuka dari HP.

**v1 selesai bila:** satu shift penuh bisa dijalankan dan **jumlah uang fisik di laci = `counted`**.

---

## 9. Aturan kenyamanan & kecepatan kasir

Ini bukan hiasan — setiap poin di bawah pernah jadi keluhan nyata atau dihindari sejak awal.

1. **Keyboard-first.** Setelah PIN, fokus langsung ke kolom cari. `Enter` = tambah item pertama
   hasil cari, `F2` = bayar, `Esc` = tutup dialog, `+/-` = qty, `Del` = hapus baris. Kasir warkop
   tidak selalu pakai mouse.
2. **Kode menu dipertahankan** (`MN-KOPI-03`). Kasir lama hafal kode; pencarian harus match kode,
   nama, **dan** alias yang bisa diisi owner.
3. **Target sentuh ≥ 48 px**, grid 3–4 kolom di tablet 10", tombol bayar besar, tanpa animasi berat.
4. **Kirim katalog sekali.** 39 item + harga = beberapa KB: sematkan sebagai JSON di halaman kasir
   supaya grid muncul instan tanpa round-trip; refresh hanya saat `settings.catalog_version`
   berubah.
5. **Latency budget:** dari tap item sampai cart berubah **< 100 ms** di LAN; dari tap "Bayar"
   sampai struk mulai keluar **≤ 2 s**. Ukur dari log durasi handler, bukan feeling.
6. **Jangan ada spinner di jalur bayar.** Tombol bayar idempoten (anti dobel nota): disable +
   token sekali-pakai dari server.
7. **Keypad nominal cepat**: 5k/10k/20k/50k/100k/uang pas + backspace; kembalian dihitung realtime
   saat mengetik. Jangan paksa kasir mengetik `30000` untuk tagihan Rp 26.000.
8. **Satu nota per layar di v1.** Jangan bikin multi-nota/tab paralel — di sistem lama itu sumber
   bug sinkronisasi cart.
9. **Printer selalu kelihatan:** badge status cetak + tombol **Tes Printer** dan **Cetak Ulang**
   di header kasir, bukan di halaman settings yang jarang dibuka. Cetak gagal → masuk log + badge.
10. **Struk & EOD satu renderer**: satu fungsi menghasilkan byte ESC/POS *dan* HTML untuk layar/cetak
    ulang dari data yang sama (di sistem lama layout ada dua tempat dan sudah pernah beda isi).
11. **Tutup shift: angka besar, sekali lihat.** `opening / penjualan cash / penjualan QRIS /
    counted / selisih`, tombol hitung cepat, dan **tanpa auto-isi** (§6.3).
12. **Nomor nota setelah bayar ditampilkan besar** (kasir membacakan/menulis), plus tombol cetak ulang.
13. **Laporan ramah HP** dan angkanya sama persis dengan EOD. Owner memeriksa dari rumah.
14. **Jangan taruh fitur non-kasir di layar kasir.** Tidak ada settings, tidak ada laporan,
    tidak ada riwayat panjang di jalur jualan.
15. **Tablet:** mode kiosk (fullscreen) + setel tablet "jangan tidur" saat shift. Jangan andalkan
    `WakeLock`/service worker — butuh secure context (HTTPS) yang tidak kita punya di LAN.
16. **Fault isolation:** printer mati, WiFi kedip, router rebooting → nota **tetap** tersimpan di
    server lokal. Tidak ada alur yang membatalkan penjualan karena alat.

---

## 10. Cetak

- Target: `settings.printer_host` + `settings.printer_port` (default `192.168.1.254:9100` — IP
  MikroTik dengan NAT port 9100 ke printer; disengaja karena colokan modem habis).
- Lebar kertas: `settings.paper_cols` (48 = 80 mm, 32 = 58 mm). **Belum dikonfirmasi** mana yang
  terpasang (§14 no.6) — jangan hard-code di luar `print/`.
- Tes Printer **wajib** ada, hasilnya ditampilkan (OK / timeout / refused). Timeout = masalah
  NAT/printer, **bukan** kode POS.
- Setiap kegagalan cetak ditulis ke log + badge UI. "Tersimpan" ≠ "bisa cetak".

---

## 11. Voucher hotspot MikroTik (keputusan owner masih terbuka)

Fakta dari sistem lama: item `MN-HOTSPOT-2JAM` (kind `service`) dijual di POS → `on_submit` menerbitkan
voucher ke RouterOS → kode voucher dicetak di blok **VOUCHER HOTSPOT** di struk. Konfigurasi per item:
`items.hotspot_enabled`, `mikrotik_profile`, `hotspot_duration_minutes`, `hotspot_validity_minutes`.
Kredensial router = setting, **jangan pernah** masuk kode/log/commit.

Sampai owner memutuskan (§14 no.1): kolom `hotspot_*` tinggal 0/NULL dan tabel `vouchers` kosong.
Jangan mengisi kredensial, jangan memanggil router.

---

## 12. Verifikasi & definition of done

Setiap perubahan yang menyentuh uang/stok wajib punya tes:

```rust
cash_change_is_exact()                  // 26.000 bayar 50.000 → 24.000
sale_deducts_recipe_stock_once()        // 1 nota = 1 set stock_moves, retry tidak dobel
void_reverses_stock_and_money()         // saldo balik, nota tetap ada
expected_cash_excludes_qris()
counted_zero_is_not_autofill()
sale_number_is_unique_under_two_cashiers()
migrate_from_empty_db()                 // sudah ada di tests/lifecycle.rs
```

Sebelum rilis: 1 nota cash + 1 nota QRIS + 1 add-on → tutup shift → **hitung uang fisik di laci**.
Struk: tidak ada baris > `paper_cols` kolom.

---

## 13. Jebakan yang sudah dibayar mahal (dari sistem lama)

1. **Cetak "sukses" ≠ kertas keluar.** Printer di balik NAT; saat NAT belum dipasang, port 9100
   tertutup dan auto-print mati **diam-diam** sejak 11 Okt — tanpa error di UI, tanpa jejak di log.
2. **Satu sumber pengaturan printer.** Semua jalur cetak membaca `printer_config()` yang sama.
3. **PWA/service worker = blank screen.** Jangan ditambahkan.
4. **Build sukses ≠ tampilan benar.** Block HTML salah kedalaman tetap compile — cek visual.
5. **Resep menunjuk bahan tidak ada = stok tidak pernah potong, HPP Rp 0.** Impor harus gagal keras.
6. **Kolom yang dikira ada ternyata tidak ada** (`selling_or_buying` di ERPNext v15): error tiap
   hari selama 2 bulan tanpa ada yang lihat. Di sini skema milik kita sendiri + tes migrasi dari DB
   kosong.
7. **Jangan mencampur `opening` dan `expected`** di laporan — kasir membaca dua angka itu sebagai
   angka yang sama dan salah mengetik.
8. **Harga 0 harus bertanya**, bukan menjual gratis.

---

## 14. Keputusan

**Sudah dikunci:** satu outlet · SQLite lokal · HTMX tanpa npm · printer lewat NAT `192.168.1.254:9100`
· QRIS statis · tanpa kas keluar · akuntansi/GL, wallet/loyalty, promo/offer/coupon, credit, partial
payment, meja/KDS, batch/serial, bundle = tidak dibangun · tidak ada item yang di-disable · menu tidak
dilacak sebagai stok (menu item ERPNext tidak diikuti) · riwayat nota lama tidak diimpor per baris
(cukup agregat bulanan di `legacy_totals`).

**Terbuka — tanya owner, jangan menebak:**

1. Voucher hotspot MikroTik: diport ke sini di v1, atau tetap di ERPNext?
2. Master data: cukup impor CSV/CLI di v1, atau sekalian halaman "Menu & Harga"?
3. Tablet kedua / dua kasir bersamaan?
4. Stok negatif: blokir penjualan atau izinkan + tandai?
5. Waste Log (bahan terbuang) dipakai atau tidak?
6. Lebar kertas printer: 58 mm (32 kolom) atau 80 mm (48 kolom)?

---

## 15. Migrasi & masa paralel

1. Opname awal 44 bahan (tanggal cutover dicatat di `ref`).
2. Jalankan RustPOS sebagai kasir, **ERPNext tetap hidup** sebagai pembanding laporan.
3. Bandingkan per shift: jumlah nota, omzet, penjualan cash, penjualan QRIS. Cocok = lanjut;
   tidak cocok = perbaiki dulu, jangan disable ERPNext.
4. Setelah 1 minggu cocok: `systemctl disable` bench. **DB ERPNext jangan dihapus** (arsip pajak/riwayat).
5. Backup harian: `sqlite3 .backup` + rotasi 30 hari (lihat `ops/`).

---

## 16. Git & cara kerja

- Branch per fitur: `feat/kasir-grid`, `fix/struk-58mm` → merge ke `main`. Jangan commit di `main`.
- Pesan commit Inggris, awalan `feat|fix|docs|chore`, jelaskan **kenapa**.
- `cargo fmt && cargo clippy --all-targets -- -D warnings` sebelum commit. Jangan commit `*.db`.
- Satu sesi kerja = satu laporan singkat: apa yang jalan, apa yang belum, angka verifikasi.
  Tanpa narasi proses.
