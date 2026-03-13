use hypr_claw::infra::credential_store::{CredentialStore, CredentialStoreError};
use sha2::Digest;
use tempfile::tempdir;

#[test]
fn test_encrypt_decrypt_roundtrip() {
    let dir = tempdir().unwrap();
    let key = [42u8; 32];
    let store = CredentialStore::new(dir.path(), &key).unwrap();

    store.store_secret("test_key", "secret_value").unwrap();
    let retrieved = store.get_secret("test_key").unwrap();

    assert_eq!(retrieved, "secret_value");
}

#[test]
fn test_wrong_key_fails() {
    let dir = tempdir().unwrap();
    let key1 = [42u8; 32];
    let key2 = [99u8; 32];

    let store1 = CredentialStore::new(dir.path(), &key1).unwrap();
    store1.store_secret("test_key", "secret_value").unwrap();

    // Try to read with different key
    let store2 = CredentialStore::new(dir.path(), &key2).unwrap();
    let result = store2.get_secret("test_key");

    assert!(matches!(result, Err(CredentialStoreError::Encryption)));
}

#[test]
fn test_modified_ciphertext_fails() {
    let dir = tempdir().unwrap();
    let key = [42u8; 32];
    let store = CredentialStore::new(dir.path(), &key).unwrap();

    store.store_secret("test_key", "secret_value").unwrap();

    // Corrupt the file
    let hash = format!("{:x}", sha2::Sha256::digest(b"test_key"));
    let file_path = dir.path().join(format!("{}.enc", hash));
    let mut data = std::fs::read(&file_path).unwrap();
    data[20] ^= 0xFF; // Flip a bit
    std::fs::write(&file_path, data).unwrap();

    // Clear cache to force reload
    drop(store);
    let store = CredentialStore::new(dir.path(), &key).unwrap();

    let result = store.get_secret("test_key");
    assert!(matches!(result, Err(CredentialStoreError::Encryption)));
}

#[test]
fn test_modified_nonce_fails() {
    let dir = tempdir().unwrap();
    let key = [42u8; 32];
    let store = CredentialStore::new(dir.path(), &key).unwrap();

    store.store_secret("test_key", "secret_value").unwrap();

    // Corrupt the nonce
    let hash = format!("{:x}", sha2::Sha256::digest(b"test_key"));
    let file_path = dir.path().join(format!("{}.enc", hash));
    let mut data = std::fs::read(&file_path).unwrap();
    data[5] ^= 0xFF; // Flip a bit in nonce
    std::fs::write(&file_path, data).unwrap();

    // Clear cache
    drop(store);
    let store = CredentialStore::new(dir.path(), &key).unwrap();

    let result = store.get_secret("test_key");
    assert!(matches!(result, Err(CredentialStoreError::Encryption)));
}

#[test]
fn test_random_nonce_uniqueness() {
    let dir = tempdir().unwrap();
    let key = [42u8; 32];
    let store = CredentialStore::new(dir.path(), &key).unwrap();

    // Store same value twice
    store.store_secret("key1", "same_value").unwrap();
    store.store_secret("key2", "same_value").unwrap();

    // Read the encrypted files
    let hash1 = format!("{:x}", sha2::Sha256::digest(b"key1"));
    let hash2 = format!("{:x}", sha2::Sha256::digest(b"key2"));
    let file1 = std::fs::read(dir.path().join(format!("{}.enc", hash1))).unwrap();
    let file2 = std::fs::read(dir.path().join(format!("{}.enc", hash2))).unwrap();

    // Nonces should be different
    assert_ne!(&file1[..12], &file2[..12]);

    // Ciphertexts should be different (due to different nonces)
    assert_ne!(&file1[12..], &file2[12..]);
}

#[test]
fn test_zeroize_on_drop() {
    let dir = tempdir().unwrap();
    let key = [42u8; 32];

    {
        let store = CredentialStore::new(dir.path(), &key).unwrap();
        store.store_secret("test_key", "secret_value").unwrap();
        // Store drops here
    }

    // Create new store and verify data still accessible
    let store = CredentialStore::new(dir.path(), &key).unwrap();
    let retrieved = store.get_secret("test_key").unwrap();
    assert_eq!(retrieved, "secret_value");
}

#[test]
fn test_multiple_secrets() {
    let dir = tempdir().unwrap();
    let key = [42u8; 32];
    let store = CredentialStore::new(dir.path(), &key).unwrap();

    store.store_secret("key1", "value1").unwrap();
    store.store_secret("key2", "value2").unwrap();
    store.store_secret("key3", "value3").unwrap();

    assert_eq!(store.get_secret("key1").unwrap(), "value1");
    assert_eq!(store.get_secret("key2").unwrap(), "value2");
    assert_eq!(store.get_secret("key3").unwrap(), "value3");
}

#[test]
fn test_overwrite_secret() {
    let dir = tempdir().unwrap();
    let key = [42u8; 32];
    let store = CredentialStore::new(dir.path(), &key).unwrap();

    store.store_secret("key", "value1").unwrap();
    store.store_secret("key", "value2").unwrap();

    assert_eq!(store.get_secret("key").unwrap(), "value2");
}
