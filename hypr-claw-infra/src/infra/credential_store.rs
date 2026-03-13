use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use parking_lot::Mutex;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use zeroize::Zeroize;

#[derive(Error, Debug)]
pub enum CredentialStoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Encryption error")]
    Encryption,

    #[error("Credential not found: {0}")]
    NotFound(String),
}

struct EncryptedBlob {
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

pub struct CredentialStore {
    store_path: PathBuf,
    cipher: Aes256Gcm,
    cache: Mutex<HashMap<String, EncryptedBlob>>,
}

impl CredentialStore {
    pub fn new<P: AsRef<Path>>(
        store_path: P,
        master_key: &[u8; 32],
    ) -> Result<Self, CredentialStoreError> {
        let store_path = store_path.as_ref().to_path_buf();
        fs::create_dir_all(&store_path)?;

        let cipher = Aes256Gcm::new(master_key.into());

        Ok(Self {
            store_path,
            cipher,
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn store_secret(&self, name: &str, value: &str) -> Result<(), CredentialStoreError> {
        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|_| CredentialStoreError::Encryption)?;

        // Store nonce + ciphertext
        let secret_path = self
            .store_path
            .join(format!("{}.enc", self.hash_name(name)));
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&secret_path)?;

        file.write_all(&nonce_bytes)?;
        file.write_all(&ciphertext)?;
        file.sync_all()?;

        self.cache.lock().insert(
            name.to_string(),
            EncryptedBlob {
                nonce: nonce_bytes,
                ciphertext,
            },
        );

        Ok(())
    }

    pub fn get_secret(&self, name: &str) -> Result<String, CredentialStoreError> {
        let blob = {
            let cache = self.cache.lock();
            if let Some(blob) = cache.get(name) {
                EncryptedBlob {
                    nonce: blob.nonce,
                    ciphertext: blob.ciphertext.clone(),
                }
            } else {
                drop(cache);
                self.load_from_disk(name)?
            }
        };

        let nonce = Nonce::from_slice(&blob.nonce);

        let mut plaintext = self
            .cipher
            .decrypt(nonce, blob.ciphertext.as_ref())
            .map_err(|_| CredentialStoreError::Encryption)?;

        let result =
            String::from_utf8(plaintext.clone()).map_err(|_| CredentialStoreError::Encryption)?;

        // Zeroize plaintext
        plaintext.zeroize();

        Ok(result)
    }

    pub fn delete_secret(&self, name: &str) -> Result<(), CredentialStoreError> {
        let secret_path = self
            .store_path
            .join(format!("{}.enc", self.hash_name(name)));

        if secret_path.exists() {
            fs::remove_file(&secret_path)?;
        }

        self.cache.lock().remove(name);

        Ok(())
    }

    fn load_from_disk(&self, name: &str) -> Result<EncryptedBlob, CredentialStoreError> {
        let secret_path = self
            .store_path
            .join(format!("{}.enc", self.hash_name(name)));

        if !secret_path.exists() {
            return Err(CredentialStoreError::NotFound(name.to_string()));
        }

        let mut file = File::open(&secret_path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        if data.len() < 12 {
            return Err(CredentialStoreError::Encryption);
        }

        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&data[..12]);
        let ciphertext = data[12..].to_vec();

        let blob = EncryptedBlob { nonce, ciphertext };

        self.cache.lock().insert(
            name.to_string(),
            EncryptedBlob {
                nonce: blob.nonce,
                ciphertext: blob.ciphertext.clone(),
            },
        );

        Ok(blob)
    }

    fn hash_name(&self, name: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(name.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

impl Drop for CredentialStore {
    fn drop(&mut self) {
        // Zeroize cache on drop
        self.cache.lock().clear();
    }
}
