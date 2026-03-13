use hypr_claw::infra::lock_manager;
use hypr_claw::infra::session_store::SessionStore;
use serde_json::json;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_load_empty_session() {
    let temp = TempDir::new().unwrap();
    let store = SessionStore::new(temp.path()).unwrap();

    let messages = store.load("test_session").unwrap();
    assert_eq!(messages.len(), 0);
}

#[test]
fn test_append_and_load() {
    let temp = TempDir::new().unwrap();
    let store = SessionStore::new(temp.path()).unwrap();

    let msg1 = json!({"role": "user", "content": "hello"});
    let msg2 = json!({"role": "assistant", "content": "hi"});

    store.append("test_session", &msg1).unwrap();
    store.append("test_session", &msg2).unwrap();

    let messages = store.load("test_session").unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0], msg1);
    assert_eq!(messages[1], msg2);
}

#[test]
fn test_save_overwrites() {
    let temp = TempDir::new().unwrap();
    let store = SessionStore::new(temp.path()).unwrap();

    let msg1 = json!({"id": 1});
    store.append("test_session", &msg1).unwrap();

    let new_messages = vec![json!({"id": 2}), json!({"id": 3})];
    store.save("test_session", &new_messages).unwrap();

    let messages = store.load("test_session").unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["id"], 2);
    assert_eq!(messages[1]["id"], 3);
}

#[test]
fn test_concurrent_appends() {
    let temp = TempDir::new().unwrap();
    let store = Arc::new(SessionStore::new(temp.path()).unwrap());
    let lock_manager = Arc::new(lock_manager::LockManager::new(Duration::from_secs(5)));

    let mut handles = vec![];

    for i in 0..10 {
        let store_clone = Arc::clone(&store);
        let lock_manager_clone = Arc::clone(&lock_manager);
        let handle = thread::spawn(move || {
            let _lock = lock_manager_clone.acquire("concurrent_session").unwrap();
            let msg = json!({"thread": i});
            store_clone.append("concurrent_session", &msg).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let messages = store.load("concurrent_session").unwrap();
    assert_eq!(messages.len(), 10);
}

#[test]
fn test_invalid_session_key() {
    let temp = TempDir::new().unwrap();
    let store = SessionStore::new(temp.path()).unwrap();

    assert!(store.load("../etc/passwd").is_err());
    assert!(store.load("test/session").is_err());
    assert!(store.load("").is_err());
}

#[test]
fn test_corrupted_line_handling() {
    let temp = TempDir::new().unwrap();
    let store = SessionStore::new(temp.path()).unwrap();

    let msg1 = json!({"id": 1});
    store.append("test_session", &msg1).unwrap();

    // Manually append corrupted line
    use std::fs::OpenOptions;
    use std::io::Write;
    let path = temp.path().join("test_session.jsonl");
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(file, "{{invalid json").unwrap();

    let msg2 = json!({"id": 2});
    store.append("test_session", &msg2).unwrap();

    let messages = store.load("test_session").unwrap();
    assert_eq!(messages.len(), 2); // Corrupted line skipped
    assert_eq!(messages[0]["id"], 1);
    assert_eq!(messages[1]["id"], 2);
}

#[test]
fn test_large_session() {
    let temp = TempDir::new().unwrap();
    let store = SessionStore::new(temp.path()).unwrap();

    for i in 0..1000 {
        let msg = json!({"index": i, "data": "x".repeat(100)});
        store.append("large_session", &msg).unwrap();
    }

    let messages = store.load("large_session").unwrap();
    assert_eq!(messages.len(), 1000);
    assert_eq!(messages[999]["index"], 999);
}
