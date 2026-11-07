-- Add migration script here
CREATE TABLE
    device (
        sn VARCHAR(128) NOT NULL,
        device_type VARCHAR(64) NOT NULL,
        code VARCHAR(128) NOT NULL,
        system_ver VARCHAR(32) NOT NULL,
        iemi VARCHAR(32) NULL,
        meid VARCHAR(32) NULL,
        iccid VARCHAR(32) NULL,
        mem VARCHAR(32) NULL,
        app_id VARCHAR(32) NULL,
        uid VARCHAR(64) NULL,
        is_init INTEGER NOT NULL,
        language_init INTEGER NOT NULL,
        password TEXT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (sn)
    );

CREATE INDEX device_sn_idx ON device (sn);