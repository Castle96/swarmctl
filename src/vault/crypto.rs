use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng},
};
use argon2::Argon2;
use rand::RngCore;
use zeroize::Zeroize;

use super::models::{NONCE_LEN, SALT_LEN};

pub fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .expect("argon2 key derivation failed");
    key
}

pub fn encrypt(plaintext: &[u8], password: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let mut salt = vec![0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);

    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);

    let key = derive_key(password, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    let mut key_zeroed = key;
    key_zeroed.zeroize();

    Ok((salt, nonce_bytes.to_vec(), ciphertext))
}

pub fn decrypt(
    salt: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    password: &str,
) -> anyhow::Result<Vec<u8>> {
    let key = derive_key(password, salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
    let nonce = Nonce::from_slice(nonce);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("Decryption failed — wrong password?"))?;

    let mut key_zeroed = key;
    key_zeroed.zeroize();

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_recovers_plaintext() {
        let plaintext = b"swarm-token-abc123";
        let (salt, nonce, ciphertext) = encrypt(plaintext, "hunter2").unwrap();
        let decrypted = decrypt(&salt, &nonce, &ciphertext, "hunter2").unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn roundtrip_handles_empty_and_binary_payloads() {
        for payload in [b"".as_slice(), b"\x00\xff\xfe".as_slice()] {
            let (salt, nonce, ciphertext) = encrypt(payload, "pw").unwrap();
            assert_eq!(decrypt(&salt, &nonce, &ciphertext, "pw").unwrap(), payload);
        }
    }

    #[test]
    fn wrong_password_fails_authentication() {
        let (salt, nonce, ciphertext) = encrypt(b"secret", "correct").unwrap();
        assert!(decrypt(&salt, &nonce, &ciphertext, "wrong").is_err());
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let (salt, nonce, mut ciphertext) = encrypt(b"secret", "pw").unwrap();
        ciphertext[0] ^= 0x01;
        assert!(decrypt(&salt, &nonce, &ciphertext, "pw").is_err());
    }

    #[test]
    fn mismatched_salt_or_nonce_is_rejected() {
        let (salt, nonce, ciphertext) = encrypt(b"secret", "pw").unwrap();
        let other_salt = encrypt(b"secret", "pw").unwrap().0;
        assert!(decrypt(&other_salt, &nonce, &ciphertext, "pw").is_err());
        assert!(decrypt(&salt, &[0u8; NONCE_LEN], &ciphertext, "pw").is_err());
    }

    #[test]
    fn encryption_is_non_deterministic() {
        // Fresh salt and nonce per call means identical plaintext must not
        // produce identical ciphertext.
        let a = encrypt(b"same", "pw").unwrap();
        let b = encrypt(b"same", "pw").unwrap();
        assert_ne!(a.0, b.0, "salt reused");
        assert_ne!(a.1, b.1, "nonce reused");
        assert_ne!(a.2, b.2, "ciphertext reused");
    }

    #[test]
    fn derived_key_depends_on_password_and_salt() {
        let salt_a = [7u8; SALT_LEN];
        let salt_b = [8u8; SALT_LEN];
        assert_ne!(derive_key("pw", &salt_a), derive_key("pw", &salt_b));
        assert_ne!(derive_key("pw", &salt_a), derive_key("pw2", &salt_a));
        assert_eq!(derive_key("pw", &salt_a), derive_key("pw", &salt_a));
    }
}
