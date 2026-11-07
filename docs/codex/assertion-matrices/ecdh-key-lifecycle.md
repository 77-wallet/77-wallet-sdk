# ECDH client key lifecycle

Scope: process-local client key generation; wire format and KDF unchanged.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Independent instances | Different public keys | independent_instances_have_different_public_keys |
| SN update and repeated handshake | Same client public key | public_key_is_stable_across_identity_and_handshake_updates |
| Two peers exchange public keys | Bidirectional decrypt and verify succeed; another client cannot decrypt or verify; tampering fails | peers_exchange_messages_and_reject_other_sessions_and_tampering |

Use only generated local keys and synthetic messages. Real backend restart and
MQTT reconnect behavior require release integration validation; local tests do
not assert server implementation compatibility.
