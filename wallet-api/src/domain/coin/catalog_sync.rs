use crate::{context::Context, error::service::ServiceError, infrastructure::parse_utc_with_error};
use chrono::{DateTime, Duration, Utc};
use wallet_database::{
    CoreDbPool,
    repositories::{coin::CoinRepo, config::ConfigRepo},
};
use wallet_transport_backend::CoinInfo;

const KEY: &str = "ordinary_catalog_progress_v1";
const OVERLAP_SECONDS: i64 = 2;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Progress {
    created: DateTime<Utc>,
    updated: DateTime<Utc>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct State {
    cursor: Option<Progress>,
}

pub(crate) struct Batch {
    pub coins: Vec<CoinInfo>,
    pub progress: Option<Progress>,
}

pub(crate) async fn fetch(ctx: &Context, pool: &CoreDbPool) -> Result<Batch, ServiceError> {
    let saved = ConfigRepo::find_by_key(KEY, pool).await?;
    let initial = if let Some(saved) = saved {
        wallet_utils::serde_func::serde_from_str::<State>(&saved.value)?.cursor
    } else {
        // Old updated_at was persisted incorrectly. Bootstrap only from the
        // latest server creation, deliberately leaving untouched history alone.
        let cursor = CoinRepo::last_coin(pool, true)
            .await?
            .filter(|row| row.created_at > DateTime::<Utc>::default())
            .map(|row| Progress { created: row.created_at, updated: row.created_at });
        // Freeze the starting point before metadata can change. A failed later
        // stage must not bootstrap a newer cursor from partially completed work.
        let value = wallet_utils::serde_func::serde_to_string(&State { cursor: cursor.clone() })?;
        ConfigRepo::upsert(KEY, &value, Some(1), pool).await?;
        cursor
    };
    let api = ctx.get_global_backend_api();
    let mut coins = if let Some(progress) = &initial {
        let created = progress
            .created
            .checked_sub_signed(Duration::seconds(OVERLAP_SECONDS))
            .unwrap_or(progress.created)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let updated = progress
            .updated
            .checked_sub_signed(Duration::seconds(OVERLAP_SECONDS))
            .unwrap_or(progress.updated)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let mut updates = api.fetch_all_tokens(None, Some(updated)).await?;
        updates.extend(api.fetch_all_tokens(Some(created), None).await?);
        updates
    } else {
        // Normal new-install catalog initialization, never a historical reset.
        api.fetch_all_tokens(None, None).await?
    };
    let mut progress = initial;
    for coin in &coins {
        let created = parse_utc_with_error(&coin.create_time)?;
        let updated = parse_utc_with_error(&coin.update_time)?;
        match &mut progress {
            Some(p) => {
                p.created = p.created.max(created);
                p.updated = p.updated.max(updated);
            }
            None => progress = Some(Progress { created, updated }),
        }
    }
    // Existing upsert deduplicates first-seen identities. Keep the freshest
    // server version first when the two independent queries overlap.
    coins.sort_by(|a, b| b.update_time.cmp(&a.update_time));
    Ok(Batch { coins, progress })
}

pub(crate) async fn complete(
    pool: &CoreDbPool,
    progress: Option<Progress>,
) -> Result<(), ServiceError> {
    let value = wallet_utils::serde_func::serde_to_string(&State { cursor: progress })?;
    // Data first, progress last. A failed save/cancellation safely replays
    // the old window instead of excluding uncommitted metadata.
    ConfigRepo::upsert(KEY, &value, Some(1), pool).await?;
    Ok(())
}
