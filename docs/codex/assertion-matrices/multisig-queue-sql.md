# Multisig queue SQL binding

Scope: production repository paths for queue listing, account-ID lookup, and
multi-account deletion.

| Scenario | Required result | Regression |
| --- | --- | --- |
| Address and chain contain apostrophes | Matching page and count are returned | `multisig_queue_repo_list_binds_address_and_chain_filters` |
| Account ID contains an apostrophe | Matching queue is returned | `multisig_queue_repo_account_id_lookup_binds_values` |
| Delete selector contains an apostrophe | Only the selected account queue is deleted | `multisig_queue_repo_multi_delete_binds_account_ids` |

The unused joined `MultisigQueueDaoV1::queue_list` method remains outside this
batch.
