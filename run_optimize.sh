#!/bin/bash

# 运行 SQLite 优化命令

# 连接到钱包数据库并执行优化命令
sqlite3 ./wallet.db "PRAGMA optimize;"
sqlite3 ./wallet.db "ANALYZE;"

echo "SQLite optimization completed!"
echo "------------------------------------"
echo "PRAGMA optimize: Rebuilds SQLite's internal statistics"
echo "ANALYZE: Updates table and index statistics"
echo "------------------------------------"
echo "This will help SQLite choose better execution plans for queries."