//! Gateway for session resolution.

use crate::interfaces::RuntimeError;

/// Resolve session key from user_id and agent_id.
///
/// # Arguments
/// * `user_id` - User identifier
/// * `agent_id` - Agent identifier
///
/// # Returns
/// Session key in format "agent_id:user_id"
///
/// # Errors
/// Returns error if user_id or agent_id is empty
pub fn resolve_session(user_id: &str, agent_id: &str) -> Result<String, RuntimeError> {
    if user_id.is_empty() || agent_id.is_empty() {
        return Err(RuntimeError::SessionError(
            "user_id and agent_id must be non-empty".to_string(),
        ));
    }

    Ok(format!("{}:{}", agent_id, user_id))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_correct_session_key_format() {
        let session_key = resolve_session("user123", "agent456").unwrap();
        assert_eq!(session_key, "agent456:user123");
    }

    #[test]
    fn test_with_different_ids() {
        assert_eq!(
            resolve_session("alice", "chatbot").unwrap(),
            "chatbot:alice"
        );
        assert_eq!(
            resolve_session("bob", "assistant").unwrap(),
            "assistant:bob"
        );
        assert_eq!(
            resolve_session("user_1", "agent_2").unwrap(),
            "agent_2:user_1"
        );
    }

    #[test]
    fn test_empty_user_id_raises_error() {
        let result = resolve_session("", "agent123");
        assert!(result.is_err());
        match result {
            Err(RuntimeError::SessionError(msg)) => {
                assert!(msg.contains("non-empty"));
            }
            _ => panic!("Expected SessionError"),
        }
    }

    #[test]
    fn test_empty_agent_id_raises_error() {
        let result = resolve_session("user123", "");
        assert!(result.is_err());
        match result {
            Err(RuntimeError::SessionError(msg)) => {
                assert!(msg.contains("non-empty"));
            }
            _ => panic!("Expected SessionError"),
        }
    }

    #[test]
    fn test_both_empty_raises_error() {
        let result = resolve_session("", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_special_characters_in_ids() {
        let session_key = resolve_session("user@example.com", "agent-v2").unwrap();
        assert_eq!(session_key, "agent-v2:user@example.com");
    }
}
