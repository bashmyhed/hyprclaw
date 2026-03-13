use hypr_claw::infra::credential_store::CredentialStore;
use std::fs;

#[test]
fn test_bootstrap_nvidia_credential_storage() {
    let test_dir = "./test_data/bootstrap_creds";
    let _ = fs::remove_dir_all(test_dir);
    fs::create_dir_all(test_dir).unwrap();

    let master_key = [42u8; 32];
    let store = CredentialStore::new(test_dir, &master_key).unwrap();

    let api_key = "nvapi-test-key-12345";
    store.store_secret("llm/nvidia_api_key", api_key).unwrap();

    let retrieved = store.get_secret("llm/nvidia_api_key").unwrap();
    assert_eq!(retrieved, api_key);

    // Verify encrypted at rest
    let files: Vec<_> = fs::read_dir(test_dir).unwrap().collect();
    assert_eq!(files.len(), 1);

    let file_content = fs::read(files[0].as_ref().unwrap().path()).unwrap();
    let api_key_bytes = api_key.as_bytes();
    assert!(!file_content
        .windows(api_key_bytes.len())
        .any(|w| w == api_key_bytes));

    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_bootstrap_credential_wrong_key_fails() {
    let test_dir = "./test_data/bootstrap_wrong_key";
    let _ = fs::remove_dir_all(test_dir);
    fs::create_dir_all(test_dir).unwrap();

    let master_key1 = [1u8; 32];
    let master_key2 = [2u8; 32];

    let store1 = CredentialStore::new(test_dir, &master_key1).unwrap();
    store1.store_secret("test", "secret").unwrap();

    let store2 = CredentialStore::new(test_dir, &master_key2).unwrap();
    let result = store2.get_secret("test");
    assert!(result.is_err());

    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_bootstrap_missing_credential() {
    let test_dir = "./test_data/bootstrap_missing";
    let _ = fs::remove_dir_all(test_dir);
    fs::create_dir_all(test_dir).unwrap();

    let master_key = [42u8; 32];
    let store = CredentialStore::new(test_dir, &master_key).unwrap();

    let result = store.get_secret("nonexistent");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));

    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_config_reset() {
    let test_dir = "./test_data/config_reset";
    let _ = fs::remove_dir_all(test_dir);
    fs::create_dir_all(test_dir).unwrap();

    let master_key = [42u8; 32];
    let store = CredentialStore::new(test_dir, &master_key).unwrap();

    store
        .store_secret("llm/nvidia_api_key", "test-key")
        .unwrap();
    assert!(store.get_secret("llm/nvidia_api_key").is_ok());

    store.delete_secret("llm/nvidia_api_key").unwrap();
    assert!(store.get_secret("llm/nvidia_api_key").is_err());

    fs::remove_dir_all(test_dir).unwrap();
}
