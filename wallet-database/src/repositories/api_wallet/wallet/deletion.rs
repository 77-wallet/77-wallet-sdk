//! Stable deletion reads and guarded atomic local deletion.
use super::{
    ApiWalletDao, ApiWalletDbPool, ApiWalletEntity, ApiWalletRepo, with_sqlite_locked_retry,
};

/// Own the first snapshot's physical reader and its pre-read commit token.
/// Closing a borrowed read transaction releases the WAL snapshot but keeps this
/// connection for the token check. The token is never refreshed on this source.
pub struct ApiWalletDeletionSource {
    connection: sqlx::pool::PoolConnection<sqlx::Sqlite>,
    version: i64,
}

/// Fixed-size fingerprint of the exact ordered (id, chain, address) cleanup facts.
/// Length-prefix strings to avoid ambiguous encodings. SHA-256 uses the existing
/// ring crypto provider; the fingerprint is ephemeral and never persisted.
#[derive(Clone)]
pub struct WalletDeletionSnapshot {
    count: u64,
    through_id: i64,
    hash: ring::digest::Context,
}

impl Default for WalletDeletionSnapshot {
    fn default() -> Self {
        Self { count: 0, through_id: 0, hash: ring::digest::Context::new(&ring::digest::SHA256) }
    }
}
impl std::fmt::Debug for WalletDeletionSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalletDeletionSnapshot")
            .field("count", &self.count)
            .field("through_id", &self.through_id)
            .finish_non_exhaustive()
    }
}

impl WalletDeletionSnapshot {
    pub fn through_id(&self) -> i64 {
        self.through_id
    }
    pub fn len(&self) -> u64 {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn include(&mut self, rows: &[crate::entities::api_account::ApiAccountDeletionRow]) {
        for row in rows {
            self.hash.update(&row.id.to_le_bytes());
            for value in [&row.chain_code, &row.address] {
                self.hash.update(&(value.len() as u64).to_le_bytes());
                self.hash.update(value.as_bytes());
            }
            self.count += 1;
        }
    }
}
impl PartialEq for WalletDeletionSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.count == other.count
            && self.hash.clone().finish().as_ref() == other.hash.clone().finish().as_ref()
    }
}
impl Eq for WalletDeletionSnapshot {}

/// Repository-owned WAL read transaction. Every read uses this connection;
/// dropping it on error/cancellation delegates rollback/connection cleanup to SQLx.
pub struct ApiWalletDeletionRead<'conn> {
    tx: sqlx::Transaction<'conn, sqlx::Sqlite>,
    through_id: i64,
    started: std::time::Instant,
}

impl ApiWalletDeletionSource {
    pub async fn begin_read(&mut self) -> Result<ApiWalletDeletionRead<'_>, crate::Error> {
        use sqlx::Acquire as _;
        let tx = self.connection.begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        ApiWalletDeletionRead::from_transaction(tx).await
    }
}

impl<'conn> ApiWalletDeletionRead<'conn> {
    async fn from_transaction(
        mut tx: sqlx::Transaction<'conn, sqlx::Sqlite>,
    ) -> Result<Self, crate::Error> {
        let started = std::time::Instant::now();
        // BEGIN is deferred; this real table query fixes the pre-backend view.
        let through_id =
            crate::dao::api_account::ApiAccountDao::account_high_watermark(tx.as_mut()).await?;
        tracing::info!(target: "wallet_deletion_perf", stage="read_snapshot_opened",
            elapsed_ms=started.elapsed().as_millis() as u64);
        Ok(Self { tx, through_id, started })
    }

    pub async fn find_by_address(
        &mut self,
        address: &str,
    ) -> Result<Option<ApiWalletEntity>, crate::Error> {
        ApiWalletDao::detail(self.tx.as_mut(), address).await
    }

    pub async fn uid_list(&mut self) -> Result<Vec<(String,)>, crate::Error> {
        ApiWalletDao::uid_list(self.tx.as_mut()).await
    }

    pub fn empty_fingerprint(&self) -> WalletDeletionSnapshot {
        WalletDeletionSnapshot { through_id: self.through_id, ..Default::default() }
    }

    pub async fn account_page(
        &mut self,
        addresses: &[&str],
        after_id: i64,
    ) -> Result<Vec<crate::entities::api_account::ApiAccountDeletionRow>, crate::Error> {
        crate::dao::api_account::ApiAccountDao::deletion_page(
            self.tx.as_mut(),
            addresses,
            after_id,
            self.through_id,
        )
        .await
    }

    /// End the old snapshot before acquiring any fresh write transaction.
    pub async fn close(self) -> Result<(), crate::Error> {
        self.tx.rollback().await.map_err(|e| crate::Error::Database(e.into()))?;
        tracing::info!(target: "wallet_deletion_perf", stage="read_snapshot_closed",
            elapsed_ms=self.started.elapsed().as_millis() as u64);
        Ok(())
    }
}

impl ApiWalletRepo {
    pub async fn deletion_source(
        pool: &ApiWalletDbPool,
    ) -> Result<ApiWalletDeletionSource, crate::Error> {
        let mut connection =
            pool.read_ref().acquire().await.map_err(|e| crate::Error::Database(e.into()))?;
        // This precedes BEGIN AND the first real table read. A commit in between
        // can only cause conservative fallback, never an undetected change.
        let version = ApiWalletDao::data_version(&mut connection).await?;
        Ok(ApiWalletDeletionSource { connection, version })
    }

    async fn scan_deletion(
        conn: &mut sqlx::SqliteConnection,
        addresses: &[&str],
        stage_name: &'static str,
    ) -> Result<WalletDeletionSnapshot, crate::Error> {
        let snapshot_started = std::time::Instant::now();
        let mut read_time = std::time::Duration::ZERO;
        let mut hash_time = std::time::Duration::ZERO;
        let mut actual = WalletDeletionSnapshot::default();
        let through_id =
            crate::dao::api_account::ApiAccountDao::account_high_watermark(&mut *conn).await?;
        let mut cursor = 0;
        loop {
            let stage = std::time::Instant::now();
            let page = crate::dao::api_account::ApiAccountDao::deletion_page(
                &mut *conn, &addresses, cursor, through_id,
            )
            .await?;
            read_time += stage.elapsed();
            let Some(last) = page.last() else {
                break;
            };
            cursor = last.id;
            let stage = std::time::Instant::now();
            actual.include(&page);
            hash_time += stage.elapsed();
        }
        tracing::info!(target: "wallet_deletion_perf", stage=stage_name, rows=actual.len(),
                elapsed_ms=snapshot_started.elapsed().as_millis() as u64,
                read_ms=read_time.as_millis() as u64, hash_ms=hash_time.as_millis() as u64);
        Ok(actual)
    }

    /// Compatibility entrypoint without a retained first-read source. Always
    /// validate current identities under the write reservation.
    pub async fn delete_wallet_data(
        pool: &ApiWalletDbPool,
        expected_wallets: &[ApiWalletEntity],
        expected_accounts: &WalletDeletionSnapshot,
    ) -> Result<(), crate::Error> {
        Self::delete_wallet_data_with_proof(pool, expected_wallets, expected_accounts, None).await
    }

    /// `source` must own the connection used to collect `expected_accounts`.
    /// Close its borrowed read transaction before passing it into this method.
    pub async fn delete_wallet_data_with_source(
        pool: &ApiWalletDbPool,
        expected_wallets: &[ApiWalletEntity],
        expected_accounts: &WalletDeletionSnapshot,
        source: ApiWalletDeletionSource,
    ) -> Result<(), crate::Error> {
        // SQLx0.8 cloned handles share inner options even in different outer Arcs.
        // Release a shared pool's source slot before acquiring it as writer.
        let proof = if std::ptr::eq(pool.read_ref().options(), pool.write_ref().options()) {
            drop(source);
            None
        } else {
            Some(source)
        };
        Self::delete_wallet_data_with_proof(pool, expected_wallets, expected_accounts, proof).await
    }

    async fn delete_wallet_data_with_proof(
        pool: &ApiWalletDbPool,
        expected_wallets: &[ApiWalletEntity],
        expected_accounts: &WalletDeletionSnapshot,
        mut source: Option<ApiWalletDeletionSource>,
    ) -> Result<(), crate::Error> {
        if expected_wallets.is_empty() {
            return Ok(());
        }
        with_sqlite_locked_retry(|| {
            // Only the first attempt owns the original connection/token. After
            // an error it is released; retries MUST scan against original facts,
            // never acquire a new connection/token and assume they are unchanged.
            let proof = source.take();
            async move { Self::finish_deletion(pool, expected_wallets, expected_accounts, proof).await }
        }).await
    }

    async fn finish_deletion(
        pool: &ApiWalletDbPool,
        expected_wallets: &[ApiWalletEntity],
        expected_accounts: &WalletDeletionSnapshot,
        proof: Option<ApiWalletDeletionSource>,
    ) -> Result<(), crate::Error> {
        let addresses: Vec<_> = expected_wallets.iter().map(|w| w.address.as_str()).collect();
        let _guard = pool.lock_write_with_metric("delete_api_wallet_data").await;
        // Take SQLite's writer reservation BEFORE rechecking the token. This
        // also excludes writers using another pool/context/process.
        let mut tx = pool
            .write_ref()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        let lock_started = std::time::Instant::now();
        tracing::info!(target: "wallet_deletion_perf", stage="writer_reserved");
        let unchanged = if let Some(mut proof) = proof {
            ApiWalletDao::data_version(&mut proof.connection).await? == proof.version
        } else {
            false
        };
        let result = async {
            if !unchanged {
                // Another commit may be unrelated. Reuse the original final
                // scan under the write reservation rather than reject safe work.
                let actual = Self::scan_deletion(tx.as_mut(), &addresses, "transaction_snapshot").await?;
                if &actual != expected_accounts {
                    return Err(crate::Error::Other(
                        "API wallet accounts changed during deletion; retry".into(),
                    ));
                }
            }
            let wallets = ApiWalletDao::physical_delete(tx.as_mut(), &addresses).await?;
            if wallets.len() != expected_wallets.len()
                || wallets.iter().any(|row| {
                    !expected_wallets.iter().any(|expected| {
                        row.id == expected.id
                            && row.address == expected.address
                            && row.uid == expected.uid
                            && row.api_wallet_type == expected.api_wallet_type
                            && row.binding_address == expected.binding_address
                    })
                })
            {
                return Err(crate::Error::Other(
                    "API wallet identity or binding changed during deletion; retry".into(),
                ));
            }
            let delete_started = std::time::Instant::now();
            let deleted =
                crate::dao::api_account::ApiAccountDao::delete_for_wallets(tx.as_mut(), &addresses)
                    .await?;
            tracing::info!(target: "wallet_deletion_perf", stage="sql_delete_accounts", rows=deleted,
                elapsed_ms=delete_started.elapsed().as_millis() as u64);
            if deleted != expected_accounts.len() {
                return Err(crate::Error::Other(
                    "API wallet account deletion count changed; retry".into(),
                ));
            }
            for wallet in wallets {
                crate::dao::address_query_state::AddressQueryStateDao::delete_by_uid(
                    tx.as_mut(),
                    &wallet.uid,
                )
                .await?;
            }
            let commit_started = std::time::Instant::now();
            tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
            tracing::info!(target: "wallet_deletion_perf", stage="commit", elapsed_ms=commit_started.elapsed().as_millis() as u64);
            Ok(())
        }.await;
        // This is the writer-connection/gate occupancy after BEGIN IMMEDIATE;
        // successful commit includes SQLite auto-checkpoint work.
        tracing::info!(target: "wallet_deletion_perf", stage="writer_occupied",
            elapsed_ms=lock_started.elapsed().as_millis() as u64, optimistic=unchanged,
            ok=result.is_ok());
        result
    }
}

#[cfg(test)]
#[path = "deletion_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "deletion_migration_timing.rs"]
mod migration_timing;
