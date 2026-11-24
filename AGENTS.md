# AGENTS.md — Kiosbin

> **Cara pakai:** baca sampai habis sebelum menulis kode. Aturan §6 (uang), §7 (stok), §9 (UX
> kasir) tidak boleh dilanggar tanpa alasan tertulis di commit message.
> **Bahasa:** kode, README, commit message = Inggris. Dokumen ops (file ini, `docs/`) = Indonesia.
> UI kasir & struk = Indonesia (pasar pertama), string dipisah supaya bisa diterjemahkan.
> `docs/DECISIONS.md` = keputusan yang sudah dikunci; jangan didebat ulang tanpa alasan baru.

Kiosbin adalah **produk**: POS self-hosted untuk warung/kafe/retail kecil, satu binary, satu file
SQLite, tanpa cloud. Warkop 3Dara adalah **tenant uji pertama** (fixture + data referensi), bukan
bagian dari kode. Jangan pernah menaruh nama toko, harga, IP printer, atau mata uang di dalam kode —
semua itu datang dari fixture (§5).

---

## 0. Status

| Sudah jalan | Belum |
|---|---|
| `migrate` (13 tabel), `import` (fixture → katalog), `check`, 13 tes hijau di Windows **dan** WSL Linux | Halaman kasir, bayar, struk, shift, laporan, voucher, layar master data |

Urutan langkah (satu commit per langkah, jangan lompat):

1. ✅ skema + migrate
2. ✅ impor katalog dari fixture
3. opname stok awal (44 bahan tenant pertama) — disalin dari hitungan fisik owner
4. PIN kasir + halaman Buka Shift
5. grid menu + cart (HTMX partial), cek visual dulu
6. bayar cash + QRIS + simpan nota + potong stok dalam **satu transaksi DB**
7. struk thermal + tombol Tes Printer → **cetak kertas nyata** sebelum lanjut
8. tutup shift + EOD
9. laporan + CSV (owner lihat dari HP)
10. voucher (kalau O3 dijawab ya)
11. master data (impor CSV / layar Menu & Harga) — wajib sebelum dipakai harian
12. `serve` + systemd + backup harian + paket rilis (O6)

---

## 1. Stack & bentuk repo

| Layer | Pilihan | Alasan |
|---|---|---|
| Bahasa | Rust (edition 2021, stable) | satu binary, tidak ada dependency hell |
| Web | `axum` + `tokio` | router + handler, cukup |
| DB | **SQLite** (`sqlx`, WAL) | satu file; backup = copy; tanpa administrasi DB |
| Frontend | **HTMX + Askama** (server-rendered) | nol npm, nol build step, nol stale bundle |
| Cetak | ESC/POS raw via `TcpStream` | sudah terbukti di sistem lama |

**Satu repo, satu binary** (`docs/DECISIONS.md` D1): UI adalah template yang di-compile ke binary,
jadi tidak ada frontend terpisah, tidak ada API yang harus di-version-kan, tidak ada CORS, tidak ada
bundel JS basi, dan yang dipasang di PC toko cuma satu file. Kalau nanti butuh klien native/WASM,
batas HTTP itu sudah jadi garis pemisahnya.

Sudah dipakai: `sqlx 0.9` (runtime-tokio/sqlite/migrate), `tokio 1`, `serde`, `serde_json`. Langkah
web akan menambah `axum 0.8`, `askama`, `tower-sessions`. **Jangan tambah dependensi lain** tanpa
alasan di commit message — ini produk yang harus gampang di-audit saat dijual.

---

## 2. Layout

```
Kiosbin/
├── AGENTS.md · README.md · docs/ (DECISIONS.md, PRODUCT.md, tenants/)
├── Cargo.toml · migrations/ · fixtures/*.seed.json
├── src/  main.rs (CLI) · db.rs (pool, migrate, clock) · money.rs · import.rs
├── tests/lifecycle.rs
└── ops/  (belum) systemd unit, backup.sh
```

`kiosbin.db` (+ `-wal`/`-shm`) tidak pernah di-commit. Migrasi yang sudah dipakai di produksi tidak
diedit — tambah file baru.

---

## 3. Invarian produk

- `kind`: `menu` | `addon` | `raw` | `service`. `raw` + `addon` (yang punya resep) punya ledger;
  `menu` tidak (`DECISIONS.md` D9).
- `price = 0` = kasir **wajib** memasukkan harga; jangan pernah menjual 0 diam-diam.
- Fixture yang mendefinisikan `kind` menang; `infer_kind()` hanya fallback untuk file yang ditulis
  tangan (D5).
- Impor bersifat **menambah**: baris dan setting yang sudah ada tidak ditimpa; `--force` untuk
  sengaja me-refresh katalog (D6).
- Resep menunjuk bahan tidak dikenal → **gagal keras + rollback** (D7).

---

## 4. Perintah kerja

Windows (git-bash) — untuk iterasi cepat:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo run -- migrate
cargo run -- import fixtures/warkop3dara.seed.json
cargo run -- check
cargo test
cargo fmt && cargo clippy --all-targets -- -D warnings
```

WSL Ubuntu 24.04 — untuk verifikasi Linux-native (target produksi). Target dir diletakkan di ext4,
bukan di `/mnt/c`, supaya kompilasi tidak lambat:

```bash
wslrun 'export PATH="$HOME/.cargo/bin:$PATH"; cd /mnt/c/Users/mansu/dev/Kiosbin; \
        CARGO_TARGET_DIR=$HOME/.cache/kiosbin-target cargo test'
```

Output yang diharapkan:

```text
migrate: ok  db=kiosbin.db  migrations=migrations
import: ok  items written=83 kept=0  recipe lines=70  uom conversions added=4  settings added=14
import: ok  items written=0 kept=83  recipe lines=0  uom conversions added=0  settings added=0
outlet = Warkop 3Dara   currency = IDR Rp   invoice prefix = W3D   paper = 48 cols
items[addon] = 6 · items[menu] = 32 · items[raw] = 44 · items[service] = 1 · recipes = 70
```

`--db PATH` atau env `KIOSBIN_DB` untuk database lain (dipakai tes).

---

## 5. Tenant & fixture (cara onboard toko baru)

Satu file JSON = satu toko: `outlet` (nama, mata uang, symbol, desimal, prefix nota, kolom kertas),
`payment_methods` (+ `kind`: cash/qris), `printing`, `uom_conversions`, `items`, `raw_materials`.
Semua opsional kecuali `items`; yang kosong jatuh ke default produk (`My Outlet`, IDR, `POS`).

```bash
cargo run -- migrate
cargo run -- import fixtures/<toko>.seed.json
```

Aturan: **jangan** hard-code apa pun dari fixture ke dalam kode, dan **jangan** mengedit fixture
tenant pertama untuk kebutuhan tenant lain — buat file baru.

---

## 6. Aturan uang (WAJIB)

1. Uang selalu integer di **minor unit** mata uang (`money.rs`). Tidak ada `f64` di jalur uang.
2. Pembulatan ke unit terkecil **saat hitung**, bukan saat simpan.
3. **Jangan auto-isi `counted` dari `expected`** — kasir wajib mengetik angka fisik. (Di sistem lama
   16 dari 20 shift mengetik `0` → tampak seperti kas kurang.)
4. `expected` cash = `opening_float + penjualan cash`. QRIS **tidak** menambah expected cash.
   Hitung ulang dari ledger setiap kali (idempoten), jangan disimpan lalu ditambah-tambah.
5. QRIS `counted` = angka settlement, diisi manual. Jangan menyimpulkan "QRIS kurang" dari nominal
   yang diketik kasir.
6. Diskon: nominal + persen di level nota. Add-on berbayar = **baris sendiri**, catatan = teks.
   Jangan menaikkan harga baris induk untuk menutupi add-on.
7. Void = `status='void'` + `stock_moves` pembalik. Jangan `DELETE`; nomor nota tidak dipakai ulang.
8. Kas keluar/belanja laci **tidak** dibangun (0 pemakaian dari 97 shift pada tenant pertama).
   Kalau nanti ada toko yang butuh: itu fitur baru dengan desain sendiri, bukan kolom kosong.

---

## 7. Aturan stok (WAJIB)

1. **Saldo = `SUM(stock_moves.qty)`.** Tidak ada kolom saldo. Satu-satunya cara saldo jadi salah
   adalah menulis saldo.
2. Langkah 3 = **opname awal** per tenant (`reason='opening'`, tanggal cutover di `ref`). Jangan
   mengimpor saldo dari sistem lama apa adanya — baseline tenant pertama sudah rusak (17 baris
   negatif, 14 bahan ≤ 0).
3. Satu penjualan = satu set `stock_moves` (per bahan resep × qty), ditulis di transaksi yang sama
   dengan nota; `sales.deducted_at` di-set di situ → retry/replay tidak dobel.
4. Konversi UOM datang dari fixture; `from = to` = faktor 1. Ledger **selalu** dalam base UOM bahan
   (`Gram`/`Ml`/`Pcs`); konversi hanya di batas input/output.
5. Stok negatif: **izinkan tapi tandai** sampai O4 dijawab owner/tenant; jangan diam-diam memblokir
   penjualan.
6. Opname koreksi = `reason='opname'` + catatan. `waste` dipakai kalau tenant memang mencatat bahan
   terbuang.
7. HPP: `items.cost = 0` berarti belum diisi. Laporan laba **wajib** bilang "HPP belum diisi",
   jangan pernah menampilkan laba 100%.

---

## 8. Alur kasir (definition of v1)

1. Buka Shift: pilih kasir + PIN, isi modal awal.
2. Kasir: grid menu per grup, cari kode/nama, tap → cart. `+ / −`, hapus, add-on, catatan.
3. Bayar: Cash (keypad + kembalian) atau QRIS (gambar statis + tombol "Sudah Bayar").
4. Simpan → struk tercetak (gagal cetak ≠ gagal transaksi; ada cetak ulang) → cart kosong.
5. Tutup Shift: per metode `opening / penjualan / counted / selisih`, cetak EOD.
6. Laporan owner: harian, menu terjual, kas per shift, stok, CSV.

**v1 selesai bila:** satu shift penuh bisa dijalankan dan **uang fisik di laci = `counted`**.

---

## 9. Aturan kenyamanan & kecepatan kasir

1. **Keyboard-first.** Setelah PIN, fokus ke kolom cari. `Enter` = tambah hasil pertama, `F2` bayar,
   `Esc` tutup dialog, `+/-` qty, `Del` hapus baris.
2. **Kode item dipertahankan** dan dicari bareng nama + alias yang bisa diisi owner.
3. **Target sentuh ≥ 48 px**, grid 3–4 kolom di tablet 10", tanpa animasi berat.
4. **Kirim katalog sekali** per sesi (beberapa KB): grid render instan, refresh hanya kalau katalog
   berubah.
5. **Budget latency:** tap item → cart < 100 ms di LAN; tap Bayar → struk mulai keluar ≤ 2 s. Ukur
   dari log durasi handler, bukan feeling.
6. **Nol spinner di jalur bayar**; tombol bayar idempoten (token sekali-pakai + disable) supaya klik
   dobel tetap satu nota.
7. **Keypad nominal cepat** (5k/10k/20k/50k/100k/uang pas) + backspace, kembalian realtime.
8. **Satu nota per layar** di v1; multi-nota paralel = sumber bug sinkronisasi cart.
9. **Printer selalu kelihatan:** badge status cetak + tombol **Tes Printer** & **Cetak Ulang** di
   header kasir, bukan di halaman settings.
10. **Satu renderer struk** yang mengeluarkan byte ESC/POS *dan* HTML dari data yang sama.
11. **Tutup shift: angka besar, sekali lihat**, tanpa auto-isi `counted`.
12. **Nomor nota tampil besar** setelah bayar + tombol cetak ulang.
13. **Laporan ramah HP**, angkanya sama dengan EOD.
14. **Jangan taruh fitur non-kasir di layar kasir.**
15. **Tablet:** mode kiosk + setelan "jangan tidur" saat shift. Jangan andalkan `WakeLock`/service
    worker — butuh HTTPS di LAN yang tidak kita punya.
16. **Fault isolation:** printer mati, WiFi kedip, router reboot → nota tetap tersimpan di server
    lokal. Tidak ada alur yang membatalkan penjualan karena alat.

---

## 10. Cetak

- Target dari `settings.printer_host` / `printer_port`. Contoh tenant pertama:
  `192.168.1.254:9100` (IP router dengan NAT ke printer — disengaja karena colokan modem habis).
  Di Kiosbin ini TCP biasa; POS tidak perlu tahu ada NAT.
- Lebar kertas dari `settings.paper_cols` (48 = 80 mm, 32 = 58 mm). Jangan hard-code di luar modul
  `print/`. Mana yang terpasang = O2.
- Tes Printer **wajib** ada dan hasilnya ditampilkan (OK / timeout / refused). Timeout = masalah
  NAT/printer, bukan kode. Setiap gagal cetak masuk log + badge.
- "Tersimpan" ≠ "bisa cetak".

---

## 11. Voucher (layanan WiFi/akses)

Item `service` dengan `items.voucher_enabled = 1` menerbitkan voucher per unit; kode tercetak di
blok voucher pada struk. Kolom generik: `voucher_profile`, `voucher_duration_minutes`,
`voucher_validity_minutes`; hasil provisioning disimpan di `vouchers` (`provider_ref` = id dari
provider). **Provider-nya bukan bagian dari core**: MikroTik/RouterOS adalah salah satu provider.
Kredensial provider = setting, tidak pernah masuk kode/log/commit. Sampai O3 dijawab, tabel
`vouchers` dibiarkan kosong dan tidak ada panggilan ke perangkat apa pun.

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
migrate_from_empty_db()                 // sudah ada (tests/lifecycle.rs)
```

Sebelum rilis: 1 nota cash + 1 nota QRIS + 1 add-on → tutup shift → **hitung uang fisik di laci**.
Struk: tidak ada baris > `paper_cols` kolom. Build & tes **hijau di Windows dan WSL** sebelum commit.

---

## 13. Jebakan yang sudah dibayar mahal

1. **Cetak "sukses" ≠ kertas keluar.** Di tenant pertama, printer di balik NAT: saat NAT belum
   dipasang, port 9100 tertutup dan auto-print mati **diam-diam** 1,5 bulan tanpa error di UI.
2. **Satu sumber pengaturan printer** untuk semua jalur cetak.
3. **PWA/service worker = blank screen.** Jangan ditambahkan.
4. **Build sukses ≠ tampilan benar.** Block HTML salah kedalaman tetap compile — cek visual.
5. **Resep menunjuk bahan tidak ada = stok tidak pernah potong, HPP 0.** Impor gagal keras (D7).
6. **Kolom yang dikira ada ternyata tidak** → error harian selama 2 bulan tanpa ada yang lihat. Di
   sini skema milik sendiri + tes migrasi dari DB kosong.
7. **Jangan mencampur `opening` dan `expected`** di laporan.
8. **Harga 0 harus bertanya**, bukan menjual gratis.
9. **Jangan simpan saldo stok.** Warisan Bin negatif adalah hasil menyimpan saldo yang tidak pernah
   direkonsiliasi.

---

## 14. Deployment & ops

- Produksi = Linux di PC toko; layanan `systemd`, `WorkingDirectory` menunjuk folder data, DB satu
  file, backup harian `sqlite3 .backup` + rotasi 30 hari.
- Tablet kasir buka `http://<ip-pc>:<port>` di WiFi yang sama; server lokal ⇒ tidak butuh PWA/offline.
- Verifikasi Linux-native lewat WSL (§4) sebelum menandai langkah selesai.
- Onboard toko baru = `migrate` + `import <fixture>` + isi settings (printer, QRIS, PIN) + test cetak.

---

## 15. Git & cara kerja

- Branch per fitur (`feat/till-grid`, `fix/receipt-58mm`) → merge `main`. Jangan commit di `main`.
- Commit Inggris, awalan `feat|fix|docs|chore`, jelaskan **kenapa**.
- `cargo fmt && cargo clippy --all-targets -- -D warnings` + tes hijau sebelum commit. Jangan commit
  `*.db`.
- Satu sesi kerja = satu laporan singkat: apa yang jalan, apa yang belum, angka verifikasi. Tanpa
  narasi proses.
