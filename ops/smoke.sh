#!/usr/bin/env bash
# Real end-to-end drive of the running till over HTTP (localhost).
set -u
BASE=http://localhost:8080
JAR=C:/Users/mansu/dev/kiosbin/jar.txt   # native path: curl is a native binary
cd /c/Users/mansu/dev/kiosbin
rm -f "$JAR"

echo "--- health ---"
curl -s "$BASE/health"; echo

echo "--- till without login ---"
curl -s -o /dev/null -w "status=%{http_code} -> %{redirect_url}\n" "$BASE/"

echo "--- login page lists cashiers ---"
curl -s -c "$JAR" "$BASE/login" | grep -o "Rina" | head -1

echo "--- wrong PIN ---"
curl -s -b "$JAR" -c "$JAR" -X POST -d 'cashier=Rina&pin=0000' -o /dev/null -w "status=%{http_code} -> %{redirect_url}\n" "$BASE/login"

echo "--- right PIN ---"
curl -s -b "$JAR" -c "$JAR" -X POST -d 'cashier=Rina&pin=1234' -o /dev/null -w "status=%{http_code} -> %{redirect_url}\n" "$BASE/login"

echo "--- open shift (modal 100.000) ---"
curl -s -b "$JAR" -c "$JAR" -X POST -d 'opening_float=100.000' -o /dev/null -w "status=%{http_code} -> %{redirect_url}\n" "$BASE/shift/open"

echo "--- till page ---"
curl -s -b "$JAR" -c "$JAR" "$BASE/" | grep -o "Kasir Rina\|Kopi 3Dara\|Keranjang\|>Semua<\|Coffee" | sort -u | tr '\n' ' '; echo

ID=$(python3 -c "import sqlite3;print(sqlite3.connect('kiosbin.db').execute(\"SELECT id FROM items WHERE code='MN-KOPI-03'\").fetchone()[0])")
echo "--- add MN-KOPI-03 (id=$ID) twice ---"
curl -s -b "$JAR" -c "$JAR" -X POST -d "item_id=$ID" "$BASE/cart/add" >/dev/null
curl -s -b "$JAR" -c "$JAR" -X POST -d "item_id=$ID" "$BASE/cart/add" | grep -o "Rp 18.000\|Rp 36.000" | tr '\n' ' '; echo

echo "--- search partial ---"
curl -s -b "$JAR" -c "$JAR" "$BASE/items?q=teh" | grep -o "MN-TEH[^\"<]*" | head -3 | tr '\n' ' '; echo

echo "--- pay cash 50.000 ---"
curl -s -b "$JAR" -c "$JAR" -X POST -d 'method=Cash&received=50.000&discount=0' -o /dev/null -w "status=%{http_code} -> %{redirect_url}\n" "$BASE/pay"

echo "--- receipt ---"
curl -s -b "$JAR" -c "$JAR" "$BASE/receipt/1" | sed -n '/<pre class="paper">/,/<\/pre>/p' | sed 's/<[^>]*>//g' | head -18

echo "--- stock moved? ---"
python3 - <<'PY'
import sqlite3
c = sqlite3.connect('kiosbin.db')
for code, qty in c.execute("""SELECT i.code, SUM(m.qty) FROM stock_moves m JOIN items i ON i.id=m.item_id
                              GROUP BY i.id ORDER BY i.code"""):
    print(f"  {code}: {qty}")
print("  sales:", c.execute("SELECT number, total FROM sales").fetchall())
PY

echo "--- close shift, counted typed ---"
curl -s -b "$JAR" -c "$JAR" -X POST -d 'counted_Cash=100.000&counted_QRIS=0' "$BASE/shift/close" | grep -o "Shift ditutup\|Rp 136.000\|Rp 36.000\|-Rp 36.000" | sort -u | tr '\n' ' '; echo

echo "--- shift row ---"
python3 -c "
import sqlite3
c=sqlite3.connect('kiosbin.db')
print(' ', c.execute('SELECT id, cashier, opening_float, status FROM shifts').fetchall())
print('  counts:', c.execute('SELECT method, counted, expected, diff FROM shift_counts').fetchall())"
