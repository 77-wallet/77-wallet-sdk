-- Add migration script here
CREATE TABLE
    multisig_account (
        id VARCHAR(64) PRIMARY KEY,
        initiator_addr VARCHAR(64) NOT NULL,
        address VARCHAR(64) NOT NULL,
        authority_addr VARCHAR(64) DEFAULT "",
        address_type VARCHAR(64) DEFAULT "",
        name VARCHAR(128) NOT NULL,
        chain_code VARCHAR(32) NOT NULL,
        threshold integer NOT NULL,
        member_num integer NOT NULL,
        pay_status integer NOT NULL,
        status integer NOT NULL,
        owner integer NOT NULL,
        salt VARCHAR NOT NULL,
        deploy_hash VARCHAR (128) DEFAULT "",
        fee_hash VARCHAR (128) DEFAULT "",
        fee_chain VARCHAR NOT NULL DEFAULT "",
        is_del integer DEFAULT 0,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP
    );