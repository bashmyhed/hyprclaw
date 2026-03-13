use hypr_claw::infra::memory_store::MemoryStore;
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

#[test]
fn test_save_and_search() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("memory.db");
    let store = MemoryStore::new(&db_path).unwrap();

    store.save_memory("user_pref", "dark mode enabled").unwrap();

    let results = store.search_memory("user").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "user_pref");
    assert_eq!(results[0].1, "dark mode enabled");
}

#[test]
fn test_update_existing_key() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("memory.db");
    let store = MemoryStore::new(&db_path).unwrap();

    store.save_memory("config", "value1").unwrap();
    store.save_memory("config", "value2").unwrap();

    let results = store.search_memory("config").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, "value2");
}

#[test]
fn test_search_by_content() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("memory.db");
    let store = MemoryStore::new(&db_path).unwrap();

    store.save_memory("key1", "contains needle here").unwrap();
    store.save_memory("key2", "no match").unwrap();

    let results = store.search_memory("needle").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "key1");
}

#[test]
fn test_search_empty_result() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("memory.db");
    let store = MemoryStore::new(&db_path).unwrap();

    store.save_memory("key1", "content1").unwrap();

    let results = store.search_memory("nonexistent").unwrap();
    assert_eq!(results.len(), 0);
}

#[test]
fn test_large_content() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("memory.db");
    let store = MemoryStore::new(&db_path).unwrap();

    let large_content = "x".repeat(100000);
    store.save_memory("large", &large_content).unwrap();

    let results = store.search_memory("large").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.len(), 100000);
}

#[test]
fn test_concurrent_writes() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("memory.db");
    let store = Arc::new(MemoryStore::new(&db_path).unwrap());

    let mut handles = vec![];

    for i in 0..10 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            let key = format!("key{}", i);
            let content = format!("content{}", i);
            store_clone.save_memory(&key, &content).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let results = store.search_memory("key").unwrap();
    assert_eq!(results.len(), 10);
}

#[test]
fn test_sql_injection_prevention() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("memory.db");
    let store = MemoryStore::new(&db_path).unwrap();

    store.save_memory("safe", "data").unwrap();

    let malicious_query = "'; DROP TABLE memory; --";
    let results = store.search_memory(malicious_query);

    assert!(results.is_ok());

    let verify = store.search_memory("safe").unwrap();
    assert_eq!(verify.len(), 1);
}

#[test]
fn test_persistence() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("memory.db");

    {
        let store = MemoryStore::new(&db_path).unwrap();
        store.save_memory("persistent", "data").unwrap();
    }

    let store2 = MemoryStore::new(&db_path).unwrap();
    let results = store2.search_memory("persistent").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, "data");
}
