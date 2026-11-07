import sqlite3, time

db_path = "benchmark_wallet.db"
wallet_type = "api"
page_size = 1000

conn = sqlite3.connect(db_path)
cursor = conn.cursor()

last_id = None
total_time = 0.0
total_rows = 0
page = 0

while True:
    if last_id:
        query = """
            SELECT id, wallet_type, wallet_id, address
            FROM wallet_addresses
            WHERE wallet_type = ? AND id > ?
            ORDER BY id
            LIMIT ?
        """
        params = (wallet_type, last_id, page_size)
    else:
        query = """
            SELECT id, wallet_type, wallet_id, address
            FROM wallet_addresses
            WHERE wallet_type = ?
            ORDER BY id
            LIMIT ?
        """
        params = (wallet_type, page_size)

    start = time.time()
    cursor.execute(query, params)
    rows = cursor.fetchall()
    duration = time.time() - start

    page += 1
    total_rows += len(rows)
    total_time += duration

    if not rows:
        break

    last_id = rows[-1][0]

conn.close()

print(f"🧪 查询 wallet_type = '{wallet_type}' 每页 {page_size} 条")
print(f"📊 总行数: {total_rows}, 页数: {page}, 总耗时: {total_time:.4f}s, 每页平均: {total_time/page:.4f}s")
