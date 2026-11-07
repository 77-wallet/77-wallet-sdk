import sqlite3, time, random
from datetime import datetime

conn = sqlite3.connect("benchmark_wallet.db")
cursor = conn.cursor()

# 建表 + 索引
cursor.execute("""
CREATE TABLE IF NOT EXISTS wallet_addresses (
    id TEXT PRIMARY KEY,
    wallet_id TEXT NOT NULL,
    wallet_type TEXT NOT NULL,
    chain_code TEXT NOT NULL,
    address_index INTEGER NOT NULL,
    address TEXT NOT NULL,
    balance TEXT,
    updated_at TEXT
);
""")
cursor.execute("CREATE INDEX IF NOT EXISTS idx_wallet_type_id ON wallet_addresses(wallet_type, id)")

now = datetime.utcnow().isoformat()
batch_size = 10000

# 插入配置
configs = [
    {"wallet_id": "wallet_api_001", "wallet_type": "api", "start": 0, "count": 9500000},
    {"wallet_id": "wallet_normal_001", "wallet_type": "normal", "start": 0, "count": 500000},
]

for config in configs:
    wallet_id = config["wallet_id"]
    wallet_type = config["wallet_type"]
    start_index = config["start"]
    total = config["count"]

    print(f"Inserting {total:,} records for wallet_type={wallet_type}...")

    for batch_start in range(start_index, start_index + total, batch_size):
        batch = []
        for i in range(batch_start, min(batch_start + batch_size, start_index + total)):
            addr = f"addr_{wallet_id}_{i:07d}"
            batch.append((
                f"{wallet_id}_{i}", wallet_id, wallet_type, "ETH", i,
                addr, str(random.uniform(0.0, 100.0)), now
            ))
        cursor.executemany("""
            INSERT INTO wallet_addresses (
                id, wallet_id, wallet_type, chain_code, address_index,
                address, balance, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """, batch)
        conn.commit()

    print(f"✅ Done inserting {wallet_id} ({wallet_type})")

conn.close()
