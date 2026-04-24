use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::rngs::OsRng;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

/// Модуль для шифрования (ECIES-подобная схема для Onion Routing)
pub struct CryptoModule {
    secret: StaticSecret,
    pub public_key: PublicKey,
}

impl Default for CryptoModule {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoModule {
    pub fn new() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public_key = PublicKey::from(&secret);
        Self { secret, public_key }
    }

    pub fn rotate_keys(&mut self) -> PublicKey {
        self.secret = StaticSecret::random_from_rng(OsRng);
        self.public_key = PublicKey::from(&self.secret);
        self.public_key
    }

    /// Шифрует данные для получателя, генерируя одноразовый ключ
    pub fn encrypt(&self, recipient_pub: &PublicKey, data: &[u8]) -> Result<Vec<u8>, String> {
        let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
        let ephemeral_pub = PublicKey::from(&ephemeral_secret);
        let shared_secret = ephemeral_secret.diffie_hellman(recipient_pub);
        
        let key = *Key::from_slice(shared_secret.as_bytes());
        let cipher = ChaCha20Poly1305::new(&key);
        
        let nonce = Nonce::from_slice(&[0u8; 12]); // Ephemeral key обеспечивает уникальность, nonce можно оставить нулевым
        let ciphertext = cipher.encrypt(nonce, data)
            .map_err(|e| format!("Ошибка шифрования: {:?}", e))?;
            
        // Приклеиваем эфемерный публичный ключ (32 байта) к началу шифротекста
        let mut result = ephemeral_pub.as_bytes().to_vec();
        result.extend(ciphertext);
        Ok(result)
    }

    /// Расшифровывает данные, используя свой StaticSecret
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < 32 {
            return Err("Слишком короткое сообщение (нет публичного ключа)".to_string());
        }
        
        let mut pub_bytes = [0u8; 32];
        pub_bytes.copy_from_slice(&data[..32]);
        let ephemeral_pub = PublicKey::from(pub_bytes);
        
        let shared_secret = self.secret.diffie_hellman(&ephemeral_pub);
        let key = *Key::from_slice(shared_secret.as_bytes());
        let cipher = ChaCha20Poly1305::new(&key);
        
        let nonce = Nonce::from_slice(&[0u8; 12]);
        cipher.decrypt(nonce, &data[32..])
            .map_err(|_| "Ошибка расшифровки (неверный ключ или поврежденные данные)".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_decryption() {
        let alice = CryptoModule::new();
        let bob = CryptoModule::new();
        
        let message = b"Hello Bob, this is Alice with a secret!";
        
        // Алиса шифрует для Боба (ей нужен публичный ключ Боба)
        let encrypted = alice.encrypt(&bob.public_key, message).unwrap();
        
        // Боб расшифровывает
        let decrypted = bob.decrypt(&encrypted).unwrap();
        
        assert_eq!(message.to_vec(), decrypted);
    }

    #[test]
    fn test_key_rotation() {
        let mut alice = CryptoModule::new();
        let bob = CryptoModule::new();
        
        let old_alice_pub = alice.public_key;
        
        alice.rotate_keys();
        assert_ne!(old_alice_pub.as_bytes(), alice.public_key.as_bytes());
        
        let msg = b"Message for new Alice";
        let encrypted = bob.encrypt(&alice.public_key, msg).unwrap();
        
        let decrypted = alice.decrypt(&encrypted).unwrap();
        assert_eq!(msg.to_vec(), decrypted);
    }
}
