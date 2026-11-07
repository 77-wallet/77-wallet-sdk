# WALLET-2977 币种列表名称来源

范围：普通钱包 getAddedCoinList / getHotCoinList；使用币种元数据，不在返回层固定 USDT 名称。

| 行为 | 证据 |
| --- | --- |
| 资产旧名称为 sui 时，已添加列表仍取 coin.name；币种名称后续变化可见 | coin_list_name_uses_current_catalog_and_preserves_stale_assets |
| USDC 名称、链合约、搜索展示标记保持，查询不回写资产名称/余额/时间 | 同上，真实 SQLite 与 domain 查询断言 |
| 缺失对应币种仍报错，不用资产旧名称兜底，不回写资产 | coin_list_name_missing_catalog_returns_error_without_changing_assets |
| 默认 Sui USDT 配置是 Tether USD；多链热门合并保留输入币种名称，包括其他目录名称 | coin_list_name_sui_usdt_default_and_hot_merge_use_catalog_names |

调用链：api/asset.rs get_coin_list → service/asset.rs get_coin_list →
domain/assets/mod.rs get_local_coin_list；api/coin.rs get_hot_coin_list →
service/coin.rs → CoinDomain::merge_coin_to_list。
没有新增查询或状态写入；保留历史 assets.name 与 schema。默认配置沿既有
启动初始化/默认币种同步流程写入 coin；不改变后端数据覆盖规则或多链排序。
未覆盖：真实 App、后端元数据质量、端到端接口联调、各链名称不一致的统一规则。
