// These integration tests must run serially because wallet-api relies on a
// global OnceCell CONTEXT.
mod bind_relation;
mod import_subaccount;
mod import_withdrawal;
mod password;
mod support;
