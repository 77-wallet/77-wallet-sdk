# Arbitrum collect-fee nonce incident and recovery boundary

Investigation: 2026-10-14 (Asia/Shanghai). User authorizes a code fix, offline tests,
independent review and read-only queries to the app RPC. Subsequent local-test
cleanup authorization is recorded below; no commits, signing or broadcast.

## Evidence

Baseline SDK HEAD: `56e7ea76feeb72dd10cb473c23b7352b5bee3d09`.
Device log SHA256: `d21376dddbfedee619a030a91c940b8c7150aaabaf97c737dba73b9fd914cba8`.
Evidence under `target/arbitrum-nonce-investigation/` is local runtime output,
not repository fixtures; raw signed bytes must not be printed in reports.

At 03:19 the same fee order `CF2110088394578280448` reserved nonce 6 before
building. Build failed: transfer value 2846760000000 wei exceeded sender balance
1945989040000 wei (this comparison excludes any extra gas requirement).
Its build slot was cleared, but its reservation remained. Subsequent attempts
consumed 7 through 57 while observed chain next remained 6. The first resulting
broadcast used 57 and was rejected as `nonce too high ... tx: 57 state: 6`.
Later fee orders repeated the failure.

The copied device transaction DB/WAL passed SQLite quick_check. This is a live
copy, not a quiesced atomic device backup, so repeat a consistent snapshot before
any recovery operation. Snapshot sender `0x37D9A67696956F67F1Bdd302A79460c1266b8F1F`
on `arbitrum` has:

- local last reservation 70;
- confirmed ordinary transfers at nonces 0, 1, 4, 5 and fee transfers at 2, 3;
- no persisted fee/collect/withdraw raw at 6 through 56 in this snapshot;
- 14 signed fee rows at 57 through 70, marked failed/finished, with nonce-too-high
  errors, no successful broadcast timestamp and no confirmed chain time.

A missing successful broadcast timestamp does **not** prove bytes never reached
a node. Failed/finished local state does **not** revoke an EVM signature.
The first code-side requests omitted the required HTTP `token` header and got401.
This was a diagnostic authentication omission, not evidence that the App or node
was failing authentication. The subsequent authenticated audit below supersedes
that limitation; its observations remain specific to their captured revision/time.

## Code repair

Claim the fee build slot before observing a nonce. Observation reads chain/local
state but does not reserve. A failed, skipped, timed-out or cancelled build cannot
advance api_nonce. Successful EVM builds atomically validate the fee identity and
lifecycle, save signed facts/status, and reserve nonce under the existing SQLite
counter writer lock. A competing allocation rolls back the candidate publication.
Existing signed raw, broadcast and receipt policies and non-EVM builds retain
their current behavior. Existing gaps stop candidate builds with
`nonce_gap_requires_recovery`; they are not silently reset.

## Authenticated code-side audit

User confirms this is their self-hosted token-protected gateway and requests
code-side testing without App/device interaction. Existing SDK code obtains a
fresh token via `BackendApi::rpc_token` (`app/rpc/token`), supplies existing
`clientId`/`AW-SEC-ID` headers to that backend, decrypts its response with configured
transport AES and sends HTTP `token` to RPC. The diagnostic mirrors that protocol
using observed app identity and local plugin configuration; token/transport keys
are kept in memory and are not printed or copied into reports. No wallet private
key/mnemonic is needed. No App source, application state or orders were modified.

The initial Python-default auth request returned403; the SDK-equivalent request
without a default Python User-Agent returned200 and a decryptable token. The
successful authenticated RPC then returned chainId `0xa4b1` (Arbitrum One42161).
This establishes the working code-side route; it does not establish the cause of
the first403 beyond that observed request difference.

At **2026-10-14 04:42:55–04:42:59 Asia/Shanghai**:

| Read | Observed result |
| --- | --- |
| Explicit block514540963, hash `0xf22fa2866089f340e4dfb41831cf4cf42e3ee160c8e0a60da656ae1f20730d72` | sender next nonce6 |
| Explicit block514540983, hash `0xaa941a5961ae15485967114b2d6b1d914c1ad0b019aaec499874d1f1ae867ba2` | sender next nonce6 |
| Pending block514540991, timestamp04:42:58; pending account query | next nonce6 |
| Balance at both explicit heights | 491149574250000 wei = 0.00049114957425 ETH |
| Hash lookups for saved fee nonces57–70 | all14 HTTP200 with `result:null`; nonce57 transient network error was retried successfully |
| Receipt lookups for57–70 | HTTP500, no success/failure chain receipt |
| Confirmed historical nonce5 control | transaction found; receipt HTTP200, status `0x1` |

No execution/confirmation of57–70 is observable in this RPC view. The saved sender
has consumed only0–5 at both fresh explicit heights. Null hash results and pending
nonce6 do not prove that no other node/signer holds a queued or replayable signed
transaction. The14 local signatures must continue to be treated as potentially
valid intents, not as revoked transactions.

Offline RLP audit of all14 saved raw records matches their recorded chain42161,
nonce57–70, destination and value. It neither re-signs/broadcasts bytes nor
cryptographically recovers the sender/verifies stored Ethereum hashes. Those
fields remain saved facts; keep original bytes and audit provenance.

### Additional RPC response issues

- `eth_getBlockByNumber("latest")` returned block514526815, timestamp03:37:51,
  at04:32 and again at04:40. The second query's `eth_blockNumber` was514540517
  and its finalized block514537230 was newer than the reported latest block.
  `eth_syncing` returnedfalse. This is a tag-result/view inconsistency consistent
  with caching or upstream routing; its server-side cause is not established.
  It does **not** justify declaring the whole node unsynchronized. Use explicit
  heights and matching block hashes/timestamps for recovery evidence.
- The nonce57 HTTP500 receipt error body includes the underlying JSON-RPC
  `result:null`; the confirmed nonce5 receipt works. Do not interpret500 as
  receipt status0, consumption of a nonce or chain execution failure. The
  gateway should preserve normal `result:null` semantics; no gateway changes
  were made in this task. Only the controlled missing receipt's500 body was
  inspected; do not assume every500 has this meaning.

Sanitized local evidence: `target/arbitrum-nonce-investigation/`
`authenticated-summary.json`, `authenticated-{chain,control,head,anchor}-facts.json`
and `signed-metadata-audit.json`. These are runtime evidence, not committed fixtures.

## Concrete recovery proposal — not implemented or executed

### Boundary

Keep api_nonce's saved floor70 and all14 signed failed order facts. Do not simply
set the counter to5, clear raw/hash, reopen terminal fee cycles or replay57–70.
The backend permits only one fee payment per collect tradeNo. New business work
must be separately authorized and must not silently pay old failed orders again.
The current signed-candidate fix deliberately rejects a local floor ahead of the
chain; it does not by itself enable this recovery.

### First controlled recovery: nonce6 only

1. **Quiesce this sender on Arbitrum across all writers.** Deploy the reviewed
   build-failure fix before resuming normal sending. Fence fee/collect/withdraw/
   ordinary transfers and any other devices/signers. Restart must not bypass the
   fence. Merely freezing the fee worker in one process is insufficient. No such
   freeze/deployment was executed in this read-only investigation.
2. **Take a fresh consistent backup and produce a dry-run manifest.** Bind sender,
   chain42161, local DB revision, counter70, every signed/raw/uncertain intent in
   the three transaction tables, backend terminal facts, new order identity and
   fresh explicit-block/pending RPC evidence. The old live-copy snapshot is not
   an execution backup. Abort if the sender/counter/intents have changed or an
   external signer/in-flight builder remains unaccounted for.
3. **Introduce a narrowly authorized unsigned-hole reservation path in code.**
   Reuse the existing shared SQLite nonce writer lock and atomic signed-fact
   commit. Unlike ordinary allocation, it may select the observed chain candidate6
   despite floor70 **only** for the approved sender/chain/order and verified hole.
   Under the writer lock, recheck the manifest/recovery fence and assert no signed,
   pending, uncertain or competing fee/collect/withdraw intent occupies6. Persist
   the new signed intent and its nonce reservation atomically; preserve floor70
   and all high-nonce facts. A scalar floor reset is not this operation.
   This path must coordinate all sending flows; a fee-only bypass with other
   allocators still running is not safe. No schema/API/recovery code was added here.
4. **Test and independently review before any real execution.** Cover stale RPC/
   failed auth, changed manifest, foreign reservation during build, duplicate
   reports, process restart, cancellation, DB rollback and broadcast uncertainty.
   Build failure reserves nothing; timeout retains the original raw/hash/nonce and
   blocks another signature for the same nonce. Bind late results to the recovery
   order/hash/nonce, not to a generic sender counter. Reuse existing test fixtures
   and reservation primitives rather than build a second normal nonce engine.
5. **Only then obtain approval for a concrete new business transaction.** Preview
   its unsigned order, destination, amount and fee cap first. Obtain explicit
   authority to create/sign that real intent before handling real keys; then show
   the hash and sanitized transaction metadata for review (keep signed raw in protected storage) and obtain separate broadcast authority. Recheck
   balances/quote and chain/pending state immediately before broadcast. Saved historical quote amounts are not
   current fee quotes. If fresh observations or facts conflict, abort, do not fall
   back to a counter reset. Observe the receipt at consistent heights; missing
   receipt/HTTP500 is uncertainty. Successful nonce6 advances the chain next to7;
   it does not finish recovery of the entire address.

### Continuing and completing recovery

- The current snapshot has no persisted raw at6–56 in the three inspected flows.
  After fresh complete audit, permit subsequent **approved** unsigned-hole work
  only within6–55, one checked nonce at a time. Do not fabricate51 filler transfers
  solely to advance the counter.
- **Stop before consuming nonce56.** Retaining56 as a gap keeps saved nonce57
  transactions ineligible. Stopping only after57 becomes the chain's next nonce
  would be too late to provide that protection: an old nonce57 signature might
  then be replayed. Do not automatically cross this boundary.
- Before releasing56, independently decide how all signed57–70 intents will be
  settled or retired, accounting for every copy/signer/backend order. Off-chain
  deletion, terminal status or a local hash denylist does not revoke signatures.
  Any approved on-chain transaction/replacement that consumes these nonces is a
  separate funds/order decision; it must not violate the single-payment contract.
  If those controls cannot be established, keep the old sender fenced and consider
  a new fee payer only after backend rebinding support/business approval is
  confirmed. That capability is not assumed here.
- Resume the normal allocator only when unresolved intents are gone/consumed and
  its floor is consistent with chain progress (for preserved floor70, next at least71).
  Safe subset recovery and complete address recovery are different checkpoints.

No real recovery SQL, nonce reset, order rewrite, signing, broadcast, gateway
change, commit or push was run. This task supplies authenticated observations and
a reviewable plan; execution requires a fresh snapshot and separate authorization.

## Superseding local-test cleanup decision (2026-10-14)

User confirms this incident is disposable local test data and explicitly approves
backup, targeted deletion of its failed records and payer nonce state, retaining
the original address and using the repaired runtime; no commit. This supersedes
the proposed production-style hole recovery for this local test only. No recovery
engine is to be implemented. Deletion does not revoke saved signatures; this
limitation was disclosed and the user chose the original address.

Current emulator-5554/com.sevenwallet.app was force-stopped before a full backup
of all five DBs and WALs. Fresh data contains23 failed fee rows57–79,23 failed
collect rows from03:19:47 through03:53:48,69 completed matching task rows, and
the payer Arbitrum nonce79. Prepare deletion only for these exact records and
the payer nonce row; preserve successful transactions, the collection-source
nonce, other addresses/chains and all remaining rows. Explicit-height/pending
RPC recheck still reports6. Remove the saved local floor, allowing the existing
nonce engine to query chain state; do not hard-code6 or clear global counters.

Evidence/backup/prepared-copy manifest under
`target/arbitrum-nonce-investigation/local-cleanup/`. Rebuild the local Android
FFI with this SDK source via an isolated Cargo patch, without modifying App or
plugin sources or publishing dependencies. Keep app stopped during replacement.
Execution and post-install verification must be recorded before claiming cleanup.

Cleanup executed on emulator-5554 after independent preparation review:23 fee,
23 collect,1 payer nonce and69 completed task rows removed; no other records
deleted. Device base/WAL/SHM originals retained privately and full host tar kept.
Installed DB hashes match prepared copies; old active WAL/SHM absent and all6
backup files match. Other3 DBs/sidecars unchanged at install.

Rebuilt Android FFI from local repaired SDK (offline, debug symbols disabled after
a disk-space failure), replaced only lib/arm64-v8a/libwallet_plugin.so in the
existing debug APK, verified all other payloads identical, signature certificate
unchanged and16KiB zip alignment valid. Installed APK hash matches prepared APK.
Startup has no observed fatal/linker errors. Quiesced post-start snapshot passes
integrity checks: transaction DB unchanged; no other rows deleted, only normal
catalog/rate/timestamp refresh and17 new background tasks. No test transaction
was created or nonce allocation triggered. No eager nonce initialization exists:
the saved floor is absent and the existing engine queries the chain on the next
build. Actual first-build allocation/signing/broadcast was deliberately not tested.
No source code commit, push or order mutation outside this local test performed.
