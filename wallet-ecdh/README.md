# wallet-ecdh

`wallet-ecdh` provides the key exchange and crypto helper flow used by the wallet stack. In the current implementation, it establishes the session shared secret and supports the request/response encryption path used by `wallet-api` and `wallet-transport-backend`.

This crate is not a standalone protocol spec. It is the implementation layer that makes the wallet backend handshake and message protection work in practice.

## Responsibilities

This crate is responsible for:

- generating and exchanging session public keys
- deriving the shared secret from the key exchange
- storing the process-level session state
- encrypting and signing outbound payloads
- verifying and decrypting inbound payloads
- exposing helper APIs that the transport layer can call directly

## Integration Flow

The detailed integration path is documented in [docs/protocols/ecdh-backend-flow.md](/Users/apple/Work/rust/77_wallet_sdk/docs/protocols/ecdh-backend-flow.md). The summary below matches the current implementation.

### 1. Handshake starts in `wallet-api`

The handshake is initiated from `wallet-api/src/service/api_wallet/wallet.rs`.

At a high level:

1. `wallet-api` checks whether the process already has an active shared secret.
2. If no shared secret exists, `wallet-api` generates the client-side public key by calling `wallet-ecdh::GLOBAL_KEY.secret_pub_key()`.
3. `wallet-api` sends that public key to the backend through the swap/init endpoint.
4. The backend returns a server public key.
5. `wallet-api` calls `wallet-ecdh::GLOBAL_KEY.set_shared_secret(...)` with the returned key.
6. Once the shared secret is ready, the rest of the wallet flow can use encrypted requests.

### 2. Outbound requests are protected in `wallet-transport-backend`

Outbound request handling lives in `wallet-transport-backend/src/api_request.rs`.

The request path is roughly:

1. Serialize the request DTO to JSON bytes.
2. Encrypt the payload with `wallet-ecdh::GLOBAL_KEY.encrypt(...)`.
3. Build the request body from the encrypted key and ciphertext.
4. Sign the request with `wallet-ecdh::GLOBAL_KEY.sign(...)`.
5. Attach `sn`, signature, and body to the backend request wrapper.

This means the transport layer depends on `wallet-ecdh` for both confidentiality and integrity.

### 3. Inbound responses are verified in `wallet-transport-backend`

Inbound response handling lives in `wallet-transport-backend/src/response/api_response.rs`.

The response path is roughly:

1. Decode the response envelope.
2. Verify the signature with `wallet-ecdh::GLOBAL_KEY.verify(...)`.
3. Decrypt the payload with `wallet-ecdh::GLOBAL_KEY.decrypt(...)`.
4. Deserialize the decrypted bytes into the business response type.

This gives the wallet stack a single place to enforce the session state and crypto checks.

## Key Concepts

- `GLOBAL_KEY`: process-level session state that stores `sn` and the shared secret.
- `ExKey`: helper API for key exchange, encryption, signing, verification, and decryption.
- `ApiBackendRequest`: outbound request wrapper that serializes, encrypts, and signs payloads.
- `ApiBackendResponse`: inbound response wrapper that verifies, decrypts, and deserializes payloads.

## State Model

The active session is stored in a process-level global state.

- `sn` is written by the upper layer during startup or manager initialization.
- `shared_secret` is written after `init_swap` succeeds.
- request signing and response decryption depend on that shared secret being present.

Once the process restarts or the session is reset, the handshake must be performed again.

## Failure Modes

- Missing handshake state results in `InvalidSharedKey`.
- Request signing may fail if the session state is not ready.
- Response verification may fail if the signature is invalid.
- Response decryption may fail if the payload does not match the active session state.
- Cross-test contamination can happen if multiple cases share the same global session state.

## Testing

Use the crate-level test target when you change key exchange or crypto behavior:

```sh
cargo test -p wallet-ecdh
```

For end-to-end validation, also run the transport layer tests that consume the shared session state:

```sh
cargo test -p wallet-transport-backend --lib
```

If you are changing the handshake contract or the request/response envelope, verify the flow against [docs/protocols/ecdh-backend-flow.md](/Users/apple/Work/rust/77_wallet_sdk/docs/protocols/ecdh-backend-flow.md) as well.

## Notes

- Protocol or parameter changes should be checked against [docs/protocols/ecdh-backend-flow.md](/Users/apple/Work/rust/77_wallet_sdk/docs/protocols/ecdh-backend-flow.md).
- This crate uses a process-level global state for the current session flow, so tests should avoid cross-case contamination.
- If you are adding new behavior here, prefer describing the integration impact in `docs/` rather than expanding this README with protocol design discussion.
