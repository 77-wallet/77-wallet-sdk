-- Internal authoritative warning snapshots, shared by active withdrawal owners.
-- Generation survives refresh failures but is discarded with the last owner.
CREATE TABLE api_app_warning_config (
    org_id TEXT NOT NULL,
    app_id TEXT NOT NULL,
    generation TEXT NOT NULL DEFAULT (lower(hex(randomblob(16)))),
    value TEXT,
    PRIMARY KEY (org_id, app_id)
);

CREATE TRIGGER api_warning_config_after_wallet_update
AFTER UPDATE OF merchant_id, app_id, status, api_wallet_type ON api_wallet
WHEN OLD.merchant_id IS NOT NULL AND OLD.app_id IS NOT NULL
BEGIN
    DELETE FROM api_app_warning_config
    WHERE org_id = OLD.merchant_id AND app_id = OLD.app_id
      AND NOT EXISTS (
          SELECT 1 FROM api_wallet
          WHERE merchant_id = OLD.merchant_id AND app_id = OLD.app_id
            AND status = 1 AND api_wallet_type = 2
      );
END;

CREATE TRIGGER api_warning_config_after_wallet_delete
AFTER DELETE ON api_wallet
WHEN OLD.merchant_id IS NOT NULL AND OLD.app_id IS NOT NULL
BEGIN
    DELETE FROM api_app_warning_config
    WHERE org_id = OLD.merchant_id AND app_id = OLD.app_id
      AND NOT EXISTS (
          SELECT 1 FROM api_wallet
          WHERE merchant_id = OLD.merchant_id AND app_id = OLD.app_id
            AND status = 1 AND api_wallet_type = 2
      );
END;
