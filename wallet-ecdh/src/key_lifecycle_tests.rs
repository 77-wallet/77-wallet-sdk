use super::ExKey;

#[test]
fn independent_instances_have_different_public_keys() {
    let first = ExKey::new();
    let second = ExKey::new();

    assert_ne!(first.secret_pub_key(), second.secret_pub_key());
}

#[test]
fn public_key_is_stable_across_identity_and_handshake_updates() {
    let client = ExKey::new();
    let server = ExKey::new();
    let original = client.secret_pub_key();

    client.set_sn("synthetic-device");
    client.set_shared_secret(&server.secret_pub_key()).unwrap();
    client.set_shared_secret(&server.secret_pub_key()).unwrap();

    assert_eq!(client.secret_pub_key(), original);
    assert_eq!(client.sn(), "synthetic-device");
}

#[test]
fn peers_exchange_messages_and_reject_other_sessions_and_tampering() {
    let client = ExKey::new();
    let server = ExKey::new();
    let other_client = ExKey::new();
    client.set_shared_secret(&server.secret_pub_key()).unwrap();
    server.set_shared_secret(&client.secret_pub_key()).unwrap();
    other_client.set_shared_secret(&server.secret_pub_key()).unwrap();

    for (sender, receiver) in [(&client, &server), (&server, &client)] {
        let plaintext = b"synthetic encrypted message";
        let encrypted = sender.encrypt(plaintext).unwrap();
        assert_eq!(receiver.decrypt(&encrypted.ciphertext, &encrypted.key).unwrap(), plaintext);
        assert!(other_client.decrypt(&encrypted.ciphertext, &encrypted.key).is_err());

        let mut signed_message = encrypted.key.clone();
        signed_message.extend_from_slice(&encrypted.ciphertext);
        let signature = sender.sign("synthetic", &signed_message).unwrap();
        receiver.verify("synthetic", &signed_message, &signature).unwrap();
        assert!(other_client.verify("synthetic", &signed_message, &signature).is_err());
        let last = signed_message.last_mut().unwrap();
        *last ^= 1;
        assert!(receiver.verify("synthetic", &signed_message, &signature).is_err());
    }
}
