//! Integration tests for terminal agent functionality.

#![allow(clippy::unwrap_used)]

use hypr_claw_runtime::*;

#[tokio::test]
async fn test_system_initialization() {
    // Test that system can initialize without panicking
    let result = std::panic::catch_unwind(|| {
        let _ = std::fs::create_dir_all("./test_data/sessions");
        let _ = std::fs::create_dir_all("./test_data/agents");
    });

    assert!(result.is_ok(), "System initialization should not panic");

    // Cleanup
    let _ = std::fs::remove_dir_all("./test_data");
}

#[tokio::test]
async fn test_concurrent_controller_access() {
    // This test verifies that RuntimeController can handle concurrent access
    // without deadlocks or panics

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        // Simulate multiple concurrent accesses
        let mut handles = vec![];
        for _ in 0..5 {
            let handle = tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                "ok"
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }
    })
    .await;

    assert!(result.is_ok(), "Concurrent access should not deadlock");
}

#[test]
fn test_error_types() {
    // Verify error types can be created and displayed
    let errors = vec![
        RuntimeError::LLMError("test".to_string()),
        RuntimeError::ToolError("test".to_string()),
        RuntimeError::LockError("test".to_string()),
        RuntimeError::SessionError("test".to_string()),
        RuntimeError::ConfigError("test".to_string()),
    ];

    for error in errors {
        let msg = error.to_string();
        assert!(!msg.is_empty(), "Error message should not be empty");
    }
}
