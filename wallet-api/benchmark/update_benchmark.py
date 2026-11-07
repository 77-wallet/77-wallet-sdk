import sqlite3, time

db_path = "benchmark_wallet.db"
wallet_type = "api"

conn = sqlite3.connect(db_path)
cursor = conn.cursor()

start_time = time.time()

cursor.execute("""
    UPDATE wallet_addresses
    SET balance = '0.000000'
    WHERE wallet_type = ?
""", (wallet_type,))

conn.commit()
duration = time.time() - start_time
conn.close()

print(f"🔄 批量更新 wallet_type = '{wallet_type}' 的 balance 为 0")
print(f"⏱ 总耗时: {duration:.4f}s")
