# PRODUCT — apa yang dijual, ke siapa, dan apa yang masih menghalangi

Dokumen ini untuk keputusan produk, bukan teknis. Aturan teknis ada di `AGENTS.md`, keputusan
terkunci di `DECISIONS.md`.

## Kalimat penawaran

> POS untuk warung, kafe, dan toko kecil: satu file aplikasi, jalan di PC toko sendiri, data tidak
> keluar dari warung, tidak ada langganan bulanan — dan stok bahan ikut terpotong otomatis dari
> resep, jadi tahu stok sisa tanpa hitung manual.

## Siapa pembelinya

1. **F&B kecil** (warung kopi, kedai, kafe, brand olahan rumahan): 50–2.000 nota/bulan, 1 outlet,
   1–2 kasir, menu bervariasi, bahan baku (gram/ml/pcs) yang mau dipantau.
2. **Toko/warung kelontong** yang butuh kasir cepat + stok barang, tanpa resep.
3. **Penjual voucher / layanan akses** (WiFi, tiket): butuh cetak kode dari struk.

Yang **bukan** target: retail multi-outlet dengan gudang berlapis, akuntansi serius, franchise,
atau bisnis yang butuh cloud multi-cabang. Ke arah itu (Postgres, server terpusat) = produk lain.

## Kenapa mereka beli (vs ERPNext/pencatat manual)

| Pembanding | Kenapa RustPOS menang |
|---|---|
| ERPNext/POSNext self-hosted | Satu binary vs bench Python+Node+MariaDB. Backup = copy 1 file. Tidak butuh admin sistem. |
| POS SaaS langganan | Tanpa biaya bulanan, tanpa kirim data penjualan ke pihak lain, tetap jalan kalau internet mati. |
| Excel/nota manual | Rekap otomatis, stok bahan terpotong per nota, uang shift ketahuan selisihnya. |
| POS gratis di HP | Layar tablet 10" + printer thermal 58/80 mm; keyboard-friendly untuk kasir cepat. |

## Model bisnis yang mungkin (belum diputuskan)

- Sekali beli (lisensi per outlet) + biaya pasang/onboarding.
- Open-core: core terbuka, edisi berbayar untuk laporan lanjutan/dashboard multi-outlet.
- Jasa: rekrut fixture = jasa input menu + resep + pelatihan kasir.

O1 di `DECISIONS.md` harus dijawab dulu: **lisensi**. Produk yang dijual tidak boleh berlisensi yang
memaksa membuka source ke pelanggan (AGPL) kecuali itu memang strateginya. Audit lisensi
dependensi + aset (HTMX, font, template) sebelum mengumumkan apa pun.

## Yang wajib ada sebelum bisa dijual

| # | Kebutuhan | Kenapa menghalangi |
|---|---|---|
| 1 | Master data tanpa editor kode (impor CSV / layar Menu & Harga) | Pemilik toko tidak akan edit JSON/SQL |
| 2 | Setup sekali jalan: buat DB, isi outlet, PIN kasir, tes printer, QRIS | Instalasi harus bisa dilakukan orang non-teknis |
| 3 | Paket rilis + cara update (O6) | Menyalin binary tidak scalable untuk puluhan toko |
| 4 | Struk yang bisa disesuaikan (nama, alamat, logo, footer) | Tiap toko beda |
| 5 | Laporan yang dipahami pemilik (harian, menu terjual, stok, kas) | Ini alasan dia bayar |
| 6 | Latihan/onboarding + fixture contoh kosong (`fixtures/example.seed.json`) | Menurunkan biaya pasang |
| 7 | Uji cetak di printer nyata 58 mm dan 80 mm | Janji "langsung jalan" harus terbukti |

## Yang sengaja TIDAK dijual (biar tidak menggelembung)

Akuntansi/GL, pajak/e-Faktur, payroll, MRP, KDS, meja/split bill, multi-outlet, wallet/loyalty,
GrabFood/GoFood, akuntansi stok FIFO. Kalau ada calon pembeli butuh: nilai sebagai proyek terpisah.
