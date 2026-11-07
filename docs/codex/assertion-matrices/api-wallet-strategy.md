# API Wallet Strategy Persistence

Scope: collect and withdrawal strategy local replacement. QA TC-CPOL-004/005
and TC-WPOL-003/004 cover editing strategy and comparing configuration; this
regression adds the interrupted local-persistence boundary. The supplied QA
XMind is unchanged.

Frontend reachability (source inspection, 2026-10-16, sf-wallet-app HEAD
`df33195d398189b978040f79be5861fcf7d678c3`): collect strategy save button calls
`CollectStrategyVM.requestSaveStrategy`; withdrawal save calls
`OutStrategyVM.onSavedStrategy`. `ApiWalletClient` passes their UID, threshold
and chain list through plugin strategy methods to WalletManager update methods.
Opening the pages calls the corresponding local-first strategy query. No input
formatter prevents a local DB write failure during these valid operations.
This is not an installed-App runtime reproduction.

| Case (strategy_save integration group) | Durable assertion | Backend assertion |
| --- | --- | --- |
| collect/withdraw replace | New threshold and complete replacement list; deleted chain gone; new list visible from reopened SQLite pool | Two saves, matching UID |
| collect/withdraw write_failure | Second-chain insert fault leaves old threshold and full old list; retry saves full new list | Failed local save follows successful backend call; explicit retry sends third save |
| collect/withdraw backend_failure | Backend rejection preserves old local strategy | Exactly two saves including rejected request |
| collect/withdraw cache_failure | First cache write failure leaves no header or partial list to mask next backend fetch; retry populates full list | Exactly two strategy fetches; subsequent local query makes no request |

`wallet-api/tests/integration/api_wallet/strategy_save/{mod.rs,support.rs}` uses
public WalletManager calls, the existing manager-without-workers testkit,
loopback encrypted backend replies, and real SQLite. An insert trigger injects
the second-chain failure. Opening another SQLite pool checks persisted rows;
this does not simulate process teardown or abrupt power loss.

The database repository reuses existing executor-based DAOs and owns one
transaction spanning header upsert, old-chain deletion, and all new inserts.
Both backend-cache and explicit-save domain paths delegate to that operation.
No HTTP call occurs while holding the SQLite transaction. SQLite serializes
writers; error/cancellation rolls back uncommitted changes through SQLx. The
existing query's two reads are not changed to a common read snapshot, and no
concurrent-query or process-kill result is claimed by these tests.

Limits: successful backend save followed by local failure still returns an
error and requires explicit retry to adopt the new configuration; there is no
distributed transaction or automatic repair of legacy partial records. No
backend response ordering, frontend UI, in-flight payment order, schema or
public API contract change is included.
