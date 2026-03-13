use hypr_claw::infra::credential_store::CredentialStore;
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

fn get_test_key() -> [u8; 32] {
    [42u8; 32]
}

#[test]
fn test_store_and_retrieve() {
    let temp = TempDir::new().unwrap();
    let store = CredentialStore::new(temp.path(), &get_test_key()).unwrap();

    store.store_secret("api_key", "secret123").unwrap();
    let retrieved = store.get_secret("api_key").unwrap();

    assert_eq!(retrieved, "secret123");
}

#[test]
fn test_update_secret() {
    let temp = TempDir::new().unwrap();
    let store = CredentialStore::new(temp.path(), &get_test_key()).unwrap();

    store.store_secret("token", "old_value").unwrap();
    store.store_secret("token", "new_value").unwrap();

    let retrieved = store.get_secret("token").unwrap();
    assert_eq!(retrieved, "new_value");
}

#[test]
fn test_delete_secret() {
    let temp = TempDir::new().unwrap();
    let store = CredentialStore::new(temp.path(), &get_test_key()).unwrap();

    store.store_secret("temp_key", "temp_value").unwrap();
    store.delete_secret("temp_key").unwrap();

    let result = store.get_secret("temp_key");
    assert!(result.is_err());
}

#[test]
fn test_not_found_error() {
    let temp = TempDir::new().unwrap();
    let store = CredentialStore::new(temp.path(), &get_test_key()).unwrap();

    let result = store.get_secret("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_encryption_verified() {
    let temp = TempDir::new().unwrap();
    let store = CredentialStore::new(temp.path(), &get_test_key()).unwrap();

    store.store_secret("secret", "plaintext").unwrap();

    let files: Vec<_> = std::fs::read_dir(temp.path()).unwrap().collect();
    assert_eq!(files.len(), 1);

    let file_path = files[0].as_ref().unwrap().path();
    let encrypted_content = std::fs::read(&file_path).unwrap();

    assert!(!encrypted_content.is_empty());
    assert_ne!(encrypted_content, b"plaintext");
}

#[test]
fn test_persistence() {
    let temp = TempDir::new().unwrap();

    {
        let store = CredentialStore::new(temp.path(), &get_test_key()).unwrap();
        store.store_secret("persistent", "data").unwrap();
    }

    let store2 = CredentialStore::new(temp.path(), &get_test_key()).unwrap();
    let retrieved = store2.get_secret("persistent").unwrap();
    assert_eq!(retrieved, "data");
}

#[test]
fn test_wrong_key_fails() {
    let temp = TempDir::new().unwrap();

    let store1 = CredentialStore::new(temp.path(), &get_test_key()).unwrap();
    store1.store_secret("secret", "value").unwrap();

    let wrong_key = [99u8; 32];
    let store2 = CredentialStore::new(temp.path(), &wrong_key).unwrap();
    let result = store2.get_secret("secret");

    assert!(result.is_err());
}

#[test]
fn test_concurrent_access() {
    let temp = TempDir::new().unwrap();
    let store = Arc::new(CredentialStore::new(temp.path(), &get_test_key()).unwrap());

    let mut handles = vec![];

    for i in 0..5 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            let name = format!("key{}", i);
            let value = format!("value{}", i);
            store_clone.store_secret(&name, &value).unwrap();
            let retrieved = store_clone.get_secret(&name).unwrap();
            assert_eq!(retrieved, value);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_special_characters() {
    let temp = TempDir::new().unwrap();
    let store = CredentialStore::new(temp.path(), &get_test_key()).unwrap();

    let special_value = "p@ssw0rd!#$%^&*(){}[]|\\:;\"'<>,.?/~`";
    store.store_secret("special", special_value).unwrap();
    let retrieved = store.get_secret("special").unwrap();

    assert_eq!(retrieved, special_value);
}
