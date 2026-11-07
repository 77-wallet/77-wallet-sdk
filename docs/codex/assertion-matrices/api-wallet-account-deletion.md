# Individual withdrawal account deletion

Scope: App withdrawal account row -> `physical_delete_api_account`.
This is an individual logical account, not deletion of the withdrawal wallet.
QA-provided XMind remains untouched.

| Scenario | Return | API database | Core database | Regression |
|---|---|---|---|---|
| Delete unconfigured logical account containing TRON + Base | success | target chains absent; other logical accounts and unrelated wallet retained | removed signer is_self=0 while another local signer remains | account_deletion_tron_uses_core_permissions_and_preserves_other_accounts |
| Delete ordinary TRON account without multisig permissions | success | target chains absent; other accounts retained | no permission required | account_deletion_tron_without_multisig_permissions_succeeds |
| Delete account referenced by withdrawal strategy | configured-account error | all account rows retained | signer membership unchanged | account_deletion_strategy_account_rejected_without_local_changes |

Evidence: `target/account-deletion/`. RED reproduces `no such table: permission`
after target rows have already been deleted, through the public manager entry,
using real migrations and valid password with no injected failures.
The fix selects the core pool for existing permission cleanup. It does not
change account guards, task sequencing or cross-database failure atomicity.
The existing integration harness suppresses task execution; backend unbind
persistence, dispatch and backend acknowledgement are not validated here.
