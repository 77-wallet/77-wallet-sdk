-- Cache only: refetch into api_wallet.db instead of importing stale binding data.
-- Literal prefix comparison (LIKE would treat underscores as wildcards).
DELETE FROM config
WHERE substr(key, 1, length('api_balance_notify_config:')) = 'api_balance_notify_config:';
