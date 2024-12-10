use crate::{
    entities::assets::{AssetsEntity, AssetsEntityWithAddressType, AssetsId},
    error::database::DatabaseError,
};
use sqlx::{Executor, Sqlite};

impl AssetsEntity {
    pub fn token_address(&self) -> Option<String> {
        if self.token_address.is_empty() {
            None
        } else {
            Some(self.token_address.clone())
        }
    }
}

impl AssetsEntity {
    pub async fn list<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql =
        "SELECT name, symbol, decimals, address, chain_code, token_address, protocol, status, balance,is_multisig,
    created_at, updated_at
    FROM assets WHERE status = 1
        AND EXISTS (
                SELECT 1
                FROM chain
                WHERE chain.chain_code = assets.chain_code
                AND chain.status = 1
            )
            AND EXISTS (
                SELECT 1
                FROM coin
                WHERE coin.chain_code = assets.chain_code
                AND coin.token_address = assets.token_address
                AND coin.symbol = assets.symbol
                AND coin.status = 1
            );";

        sqlx::query_as::<sqlx::Sqlite, AssetsEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn unactived_list<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql =
        "SELECT name, symbol, decimals, address, chain_code, token_address, protocol, status, balance,is_multisig,
    created_at, updated_at
    FROM assets WHERE status = 0
        AND EXISTS (
                SELECT 1
                FROM chain
                WHERE chain.chain_code = assets.chain_code
                AND chain.status = 1
            )
            AND EXISTS (
                SELECT 1
                FROM coin
                WHERE coin.chain_code = assets.chain_code
                AND coin.token_address = assets.token_address
                AND coin.symbol = assets.symbol
                AND coin.status = 1
            );";

        sqlx::query_as::<sqlx::Sqlite, AssetsEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_coin_assets_in_address<'a, E>(
        exec: E,
        address: Vec<String>,
        status: Option<u8>,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut sql = String::from(
            "SELECT name, symbol, decimals, address, chain_code, token_address, protocol, status, balance, is_multisig, created_at, updated_at
            FROM assets"
        );

        let mut conditions = Vec::new();
        if !address.is_empty() {
            let addresses = crate::any_in_collection(address, "','");
            conditions.push(format!(" address IN ('{}')", addresses));
        }

        if status.is_some() {
            conditions.push(" status = ?".to_string());
        }

        conditions.push(
            " EXISTS (
                SELECT 1
                FROM chain
                WHERE chain.chain_code = assets.chain_code
                AND chain.status = 1
            )"
            .to_string(),
        );

        conditions.push(
            " EXISTS (
                SELECT 1
                FROM coin
                WHERE coin.chain_code = assets.chain_code
                AND coin.token_address = assets.token_address
                AND coin.symbol = assets.symbol
                AND coin.status = 1
            )"
            .to_string(),
        );

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        let mut query = sqlx::query_as::<sqlx::Sqlite, AssetsEntity>(&sql);

        if let Some(status_value) = status {
            query = query.bind(status_value)
        }

        // 执行查询并返回结果
        query
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub(crate) async fn get_assets_by_address<'a, E>(
        exec: E,
        address: Vec<String>,
        chain_code: Option<String>,
        symbol: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntityWithAddressType>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let addresses = crate::any_in_collection(address, "','");
        let base_sql = |table_name: &str| -> String {
            format!(
                "SELECT a.name, a.symbol, a.decimals, a.address, a.chain_code, 
                a.token_address, a.protocol, a.status, a.balance, a.is_multisig, 
                a.created_at, a.updated_at, acc.address_type 
                FROM assets AS a
                JOIN {table_name} AS acc 
                ON a.address = acc.address AND a.chain_code = acc.chain_code
                WHERE a.status = 1 
                    AND EXISTS (
                        SELECT 1
                        FROM chain
                        WHERE chain.chain_code = a.chain_code
                        AND chain.status = 1
                    )
                    AND EXISTS (
                        SELECT 1
                        FROM coin
                        WHERE coin.chain_code = a.chain_code
                        AND coin.token_address = a.token_address
                        AND coin.symbol = a.symbol
                        AND coin.status = 1
                    )"
            )
        };

        let add_dynamic_conditions = |sql: &mut String| {
            if !addresses.is_empty() {
                sql.push_str(&format!(" AND a.address IN ('{}')", addresses));
            }
            if chain_code.is_some() {
                sql.push_str(" AND a.chain_code = ?");
            }
            if symbol.is_some() {
                sql.push_str(" AND a.symbol = ?");
            }
            if let Some(is_multisig) = is_multisig {
                let is_multisig_values = if is_multisig { vec![1] } else { vec![0, 2] };
                let is_multisig_str = crate::any_in_collection(is_multisig_values, "','");
                sql.push_str(&format!(" AND a.is_multisig IN ('{}')", is_multisig_str));
            }
        };

        let sql = match is_multisig {
            Some(true) => {
                let mut sql = base_sql("multisig_account");
                add_dynamic_conditions(&mut sql);
                sql
            }
            Some(false) => {
                let mut sql = base_sql("account");
                add_dynamic_conditions(&mut sql);
                sql
            }
            None => {
                let mut sql1 = base_sql("account");

                let mut sql2 = base_sql("multisig_account");
                add_dynamic_conditions(&mut sql1);
                add_dynamic_conditions(&mut sql2);
                format!("{sql1} UNION {sql2}")
            }
        };

        let mut query = sqlx::query_as::<_, AssetsEntityWithAddressType>(&sql);

        if let Some(code) = chain_code {
            query = query.bind(code);
        }

        if let Some(sym) = symbol {
            query = query.bind(sym);
        }

        query
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub(crate) async fn get_chain_assets_by_address_chain_code_symbol<'a, E>(
        exec: E,

        address: Vec<String>,
        chain_code: Option<String>,
        symbol: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let addresses = crate::any_in_collection(address, "','");
        let mut sql = "SELECT name, symbol, decimals, address, chain_code, 
        token_address, protocol, status, balance,is_multisig, created_at, updated_at FROM assets 
        WHERE status = 1
            AND EXISTS (
                SELECT 1
                FROM chain
                WHERE chain.chain_code = assets.chain_code
                AND chain.status = 1
            )
            AND EXISTS (
                SELECT 1
                FROM coin
                WHERE coin.chain_code = assets.chain_code
                AND coin.token_address = assets.token_address
                AND coin.symbol = assets.symbol
                AND coin.status = 1
            )"
        .to_string();

        if !addresses.is_empty() {
            let str = format!(" AND address in ('{}')", addresses);
            sql.push_str(&str)
        }

        if chain_code.is_some() {
            sql.push_str(" AND chain_code = ?");
        }

        if symbol.is_some() {
            sql.push_str(" AND symbol = ?");
        }

        if let Some(is_multisig) = is_multisig {
            let is_multisig = if is_multisig { vec![1] } else { vec![0, 2] };
            let is_multisig = crate::any_in_collection(is_multisig, "','");
            let str = format!(" AND is_multisig in ('{}')", is_multisig);
            sql.push_str(&str);
        }

        let mut query = sqlx::query_as::<_, AssetsEntity>(&sql);

        if let Some(code) = chain_code {
            query = query.bind(code);
        }

        if let Some(sym) = symbol {
            query = query.bind(sym);
        }

        query
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 获取资产
    pub async fn assets_by_id<'a, E>(
        exec: E,
        assets_id: &AssetsId,
    ) -> Result<Option<AssetsEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT 
                name, symbol,decimals, address, chain_code, token_address, protocol, status, balance,is_multisig,created_at, updated_at
            FROM 
                assets
            WHERE status = 1 AND address =$1 AND symbol = $2 AND chain_code = $3
                AND EXISTS (
                    SELECT 1
                    FROM chain
                    WHERE chain.chain_code = assets.chain_code
                    AND chain.status = 1
                )
                AND EXISTS (
                    SELECT 1
                    FROM coin
                    WHERE coin.chain_code = assets.chain_code
                    AND coin.token_address = assets.token_address
                    AND coin.symbol = assets.symbol
                    AND coin.status = 1
                );"#;

        let rs = sqlx::query_as::<sqlx::Sqlite, AssetsEntity>(sql)
            .bind(assets_id.address.clone())
            .bind(assets_id.symbol.clone())
            .bind(assets_id.chain_code.clone())
            .fetch_optional(exec)
            .await;

        match rs {
            Ok(rs) => Ok(rs),
            Err(_e) => Err(crate::Error::Database(DatabaseError::QueryFailed)),
        }
    }

    // 更新余额
    pub async fn update_balance<'a, E>(
        exec: E,
        assets_id: &AssetsId,
        balance: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE assets SET 
            balance = $4,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE address = $1 
        AND symbol = $2 
        AND chain_code = $3;
    "#;

        sqlx::query(sql)
            .bind(assets_id.address.to_string())
            .bind(assets_id.symbol.to_string())
            .bind(assets_id.chain_code.to_string())
            .bind(balance)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|_| crate::Error::Database(DatabaseError::UpdateFailed))
    }

    // 设置多签标识
    pub async fn update_is_multisig<'a, E>(
        exec: E,
        assets_id: &AssetsId,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE assets SET 
            is_multisig = 1,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE address = $1 AND symbol = $2 AND chain_code = $3;
    "#;

        sqlx::query(sql)
            .bind(assets_id.address.to_string())
            .bind(assets_id.symbol.to_string())
            .bind(assets_id.chain_code.to_string())
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|_| crate::Error::Database(DatabaseError::UpdateFailed))
    }

    // 插入或更新资产信息
    pub async fn upsert_assets<'a, E>(exec: E, assets: CreateAssetsVo) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let CreateAssetsVo {
            assets_id,
            name,
            decimals,
            token_address,
            protocol,
            status,
            is_multisig,
            balance,
        } = assets;

        let token_address = token_address.unwrap_or_default();
        let protocol = protocol.unwrap_or_default();

        let sql = r#"
        INSERT INTO assets
        (
            name, symbol, decimals, address, chain_code, token_address, protocol, status, balance, is_multisig, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        ON CONFLICT (symbol, address, chain_code)
        DO UPDATE SET
            balance = EXCLUDED.balance,
            status = EXCLUDED.status,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now');
    "#;

        sqlx::query(sql)
            .bind(name)
            .bind(assets_id.symbol)
            .bind(decimals)
            .bind(assets_id.address)
            .bind(assets_id.chain_code)
            .bind(token_address)
            .bind(protocol)
            .bind(status)
            .bind(balance)
            .bind(is_multisig)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|_| crate::Error::Database(DatabaseError::UpdateFailed))
    }

    pub async fn all_assets<'a, E>(
        exec: E,
        addr: Vec<String>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut sql = "select * from assets ".to_string();

        let mut conditions = Vec::new();
        conditions.push("status = 1".to_string());

        conditions.push(
            " EXISTS (
                SELECT 1
                FROM chain
                WHERE chain.chain_code = assets.chain_code
                AND chain.status = 1
            )"
            .to_string(),
        );

        conditions.push(
            " EXISTS (
                SELECT 1
                FROM coin
                WHERE coin.chain_code = assets.chain_code
                AND coin.token_address = assets.token_address
                AND coin.symbol = assets.symbol
                AND coin.status = 1
            )"
            .to_string(),
        );

        if let Some(chain_code) = chain_code {
            conditions.push(format!("chain_code = '{chain_code}'"));
        }
        if let Some(keyword) = keyword {
            conditions.push(format!("symbol LIKE '%{keyword}%'"));
        }

        if let Some(is_multisig) = is_multisig {
            let is_multisig = if is_multisig { vec![1] } else { vec![0, 2] };
            let is_multisig = crate::any_in_collection(is_multisig, "','");
            let str = format!(" is_multisig in ('{}')", is_multisig);
            conditions.push(str);
        }

        if !addr.is_empty() {
            let str = format!("address in ('{}')", addr.join("','"));
            conditions.push(str)
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        let query = sqlx::query_as::<_, AssetsEntity>(&sql);
        let result = query.fetch_all(exec).await;

        result.map_err(|e| crate::Error::Database(e.into()))
    }

    // 删除单个资产
    pub async fn delete_assets<'a, E>(assets_id: &AssetsId, exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE assets 
        SET status = $4, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE address = $1 AND symbol = $2 AND chain_code = $3
            AND EXISTS (
                SELECT 1
                FROM chain
                WHERE chain.chain_code = assets.chain_code
                AND chain.status = 1
            )
            AND EXISTS (
                SELECT 1
                FROM coin
                WHERE coin.chain_code = assets.chain_code
                AND coin.token_address = assets.token_address
                AND coin.symbol = assets.symbol
                AND coin.status = 1
            );
    "#;

        sqlx::query(sql)
            .bind(assets_id.address.to_string())
            .bind(assets_id.symbol.to_string())
            .bind(assets_id.chain_code.to_string())
            .bind(0) // Assuming 0 is the status for deletion
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|_| crate::Error::Database(DatabaseError::UpdateFailed))
    }

    pub async fn update_status<'a, E>(
        exec: E,
        chain_code: &str,
        symbol: &str,
        token_address: Option<String>,
        status: u8,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE assets
        SET status = $4, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE chain_code = $1 AND symbol = $2 AND token_address = $3
            AND EXISTS (
                SELECT 1
                FROM chain
                WHERE chain.chain_code = assets.chain_code
                AND chain.status = 1
            )
            AND EXISTS (
                SELECT 1
                FROM coin
                WHERE coin.chain_code = assets.chain_code
                AND coin.token_address = assets.token_address
                AND coin.symbol = assets.symbol
                AND coin.status = 1
            );
        "#;

        sqlx::query(sql)
            .bind(chain_code)
            .bind(symbol)
            .bind(token_address.unwrap_or_default())
            .bind(status)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|_| crate::Error::Database(DatabaseError::UpdateFailed))?;

        Ok(())
    }

    // 删除多个资产
    pub async fn delete_multi_assets<'a, E>(
        exec: E,
        assets_ids: Vec<AssetsId>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if assets_ids.is_empty() {
            return Ok(());
        }
        let placeholders = assets_ids
            .iter()
            .map(|_| "(?, ?, ?)")
            .collect::<Vec<_>>()
            .join(", ");

        // 构建 SQL 查询
        let sql = format!(
            "UPDATE assets SET status = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE (address, symbol, chain_code) IN ({})",
            placeholders
        );

        let mut query = sqlx::query(&sql);

        // 绑定参数
        for assets_id in &assets_ids {
            query = query
                .bind(&assets_id.address)
                .bind(&assets_id.symbol)
                .bind(&assets_id.chain_code);
        }

        // 执行查询
        query
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_tron_multisig_assets<'a, E>(
        address: &str,
        chain_code: &str,
        is_multisig: i8,
        exec: E,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE assets 
            SET is_multisig = ?, 
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND chain_code = ?;
        "#;

        sqlx::query(sql)
            .bind(is_multisig)
            .bind(address)
            .bind(chain_code)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|_e| crate::Error::Database(crate::DatabaseError::UpdateFailed))
    }
}

// 创建的账单的类型
#[derive(Debug)]
pub struct CreateAssetsVo {
    pub assets_id: AssetsId,
    pub name: String,
    pub decimals: u8,
    pub token_address: Option<String>,
    pub protocol: Option<String>,
    pub status: u8,
    pub is_multisig: i32,
    pub balance: String,
}

impl CreateAssetsVo {
    pub fn new(
        assets_id: AssetsId,
        decimals: u8,
        token_address: Option<String>,
        protocol: Option<String>,
        is_multisig: i32,
    ) -> Self {
        Self {
            assets_id,
            name: "name".to_string(),
            decimals,
            token_address,
            protocol,
            status: 1,
            is_multisig,
            balance: "0.00".to_string(),
        }
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    pub fn with_status(mut self, status: u8) -> Self {
        self.status = status;
        self
    }

    pub fn with_u256(
        mut self,
        balance: alloy::primitives::U256,
        decimals: u8,
    ) -> Result<Self, crate::Error> {
        let balance = wallet_utils::unit::format_to_string(balance, decimals)?;
        let balance = wallet_utils::parse_func::decimal_from_str(&balance)?;

        self.balance = balance.to_string();
        Ok(self)
    }

    pub fn with_balance(mut self, balance: &str) -> Self {
        self.balance = balance.to_string();
        self
    }

    pub fn with_protocol(mut self, protocol: Option<String>) -> Self {
        self.protocol = protocol;
        self
    }
}
