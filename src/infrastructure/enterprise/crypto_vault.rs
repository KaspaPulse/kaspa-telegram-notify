#![allow(dead_code)]
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use hex::{decode, encode};

#[allow(dead_code)]
pub struct CryptoVault {
    cipher: Aes256Gcm,
}

impl CryptoVault {
    pub fn new(hex_key: &str) -> Self {
        let key_bytes =
            decode(hex_key).expect("CRITICAL: Invalid Encryption Key Format (Must be Hex)");
        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .expect("CRITICAL: Invalid Key Length (Must be 32 bytes)");
        Self { cipher }
    }

    /// Encrypts sensitive database fields (e.g., chat history, private user data)
    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        // Generate a cryptographically secure 96-bit nonce using OS-level RNG
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| format!("Encryption failed: {:?}", e))?;

        let mut result = nonce.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(encode(result))
    }

    /// Decrypts data retrieved from the database
    pub fn decrypt(&self, encrypted_hex: &str) -> Result<String, String> {
        let data = decode(encrypted_hex).map_err(|_| "Invalid hex format")?;
        if data.len() < 12 {
            return Err("Invalid encrypted data length".to_string());
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("Decryption failed: {:?}", e))?;

        String::from_utf8(plaintext).map_err(|_| "Invalid UTF-8 after decryption".to_string())
    }
}
