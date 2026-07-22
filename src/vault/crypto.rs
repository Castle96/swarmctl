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
