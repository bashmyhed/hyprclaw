//! Message compactor for managing context window size.

use crate::interfaces::RuntimeError;
use crate::types::{Message, Role};
use serde_json::json;
use tracing::{debug, info, warn};

/// Trait for message summarization.
pub trait Summarizer: Send + Sync {
    /// Summarize a list of messages into a single string.
    fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError>;
}

/// Message compactor for token-based history management.
pub struct Compactor<S: Summarizer> {
    threshold: usize,
    summarizer: S,
}

impl<S: Summarizer> Compactor<S> {
    /// Create a new compactor.
    ///
    /// # Arguments
    /// * `threshold` - Token count threshold to trigger compaction
    /// * `summarizer` - Summarizer implementation
    pub fn new(threshold: usize, summarizer: S) -> Self {
        Self {
            threshold,
            summarizer,
        }
    }

    /// Compact messages if they exceed threshold.
    ///
    /// # Arguments
    /// * `messages` - List of messages to potentially compact
    ///
    /// # Returns
    /// Compacted message list (or original if below threshold)
    pub fn compact(&self, messages: Vec<Message>) -> Result<Vec<Message>, RuntimeError> {
        let token_count = self.estimate_tokens(&messages);

        if token_count <= self.threshold {
            debug!(
                "Token count {} below threshold {}, no compaction needed",
                token_count, self.threshold
            );
            return Ok(messages);
        }

        info!(
            "Token count {} exceeds threshold {}, compacting",
            token_count, self.threshold
        );

        crate::metrics::increment_compaction_count();

        // Split messages: older half to summarize, newer half to keep
        let split_point = messages.len() / 2;

        if split_point == 0 {
            warn!("Single message exceeds threshold, cannot compact");
            return Ok(messages);
        }

        let (older_messages, newer_messages) = messages.split_at(split_point);

        // Summarize older messages
        let summary_text = self.summarizer.summarize(older_messages)?;
        let summary_message = Message::with_metadata(
            Role::System,
            json!(summary_text),
            json!({
                "compacted": true,
                "original_count": older_messages.len()
            }),
        );

        // Return summary + newer messages
        let mut compacted = vec![summary_message];
        compacted.extend_from_slice(newer_messages);

        info!(
            "Compacted {} messages to {} messages",
            messages.len(),
            compacted.len()
        );

        Ok(compacted)
    }

    /// Estimate token count using simple length-based heuristic.
    ///
    /// # Arguments
    /// * `messages` - List of messages
    ///
    /// # Returns
    /// Estimated token count (4 characters per token)
    fn estimate_tokens(&self, messages: &[Message]) -> usize {
        let total_chars: usize = messages
            .iter()
            .map(|msg| msg.content.to_string().len())
            .sum();

        total_chars / 4
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;

    struct MockSummarizer;

    impl Summarizer for MockSummarizer {
        fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
            Ok(format!("Summary of {} messages", messages.len()))
        }
    }

    #[test]
    fn test_below_threshold_unchanged() {
        let compactor = Compactor::new(1000, MockSummarizer);
        let messages = vec![
            Message::new(Role::User, json!("Hello")),
            Message::new(Role::Assistant, json!("Hi there")),
        ];

        let result = compactor.compact(messages.clone()).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_above_threshold_compacts() {
        let compactor = Compactor::new(10, MockSummarizer);

        let messages = vec![
            Message::new(Role::User, json!("A".repeat(50))),
            Message::new(Role::Assistant, json!("B".repeat(50))),
            Message::new(Role::User, json!("C".repeat(50))),
            Message::new(Role::Assistant, json!("D".repeat(50))),
        ];

        let result = compactor.compact(messages).unwrap();

        // Should have summary + newer half
        assert_eq!(result.len(), 3); // 1 summary + 2 newer messages
        assert_eq!(result[0].role, Role::System);
        assert!(result[0]
            .content
            .to_string()
            .contains("Summary of 2 messages"));
    }

    #[test]
    fn test_preserves_recent_messages() {
        let compactor = Compactor::new(10, MockSummarizer);

        let messages = vec![
            Message::new(Role::User, json!("A".repeat(50))),
            Message::new(Role::Assistant, json!("B".repeat(50))),
            Message::new(Role::User, json!("Recent 1")),
            Message::new(Role::Assistant, json!("Recent 2")),
        ];

        let result = compactor.compact(messages).unwrap();

        // Check that recent messages are preserved
        assert_eq!(result[1].content, json!("Recent 1"));
        assert_eq!(result[2].content, json!("Recent 2"));
    }

    #[test]
    fn test_summarizer_receives_older_messages() {
        struct CapturingSummarizer {
            captured_count: std::sync::Arc<std::sync::Mutex<usize>>,
        }

        impl Summarizer for CapturingSummarizer {
            fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
                let mut count = self.captured_count.lock().unwrap();
                *count = messages.len();
                Ok("summary".to_string())
            }
        }

        let captured_count = std::sync::Arc::new(std::sync::Mutex::new(0));
        let summarizer = CapturingSummarizer {
            captured_count: captured_count.clone(),
        };
        let compactor = Compactor::new(10, summarizer);

        let messages = vec![
            Message::new(Role::User, json!("A".repeat(50))),
            Message::new(Role::Assistant, json!("B".repeat(50))),
            Message::new(Role::User, json!("C".repeat(50))),
            Message::new(Role::Assistant, json!("D".repeat(50))),
        ];

        compactor.compact(messages).unwrap();

        let count = *captured_count.lock().unwrap();
        assert_eq!(count, 2); // Should have received first half
    }

    #[test]
    fn test_single_message_exceeding_threshold() {
        let compactor = Compactor::new(10, MockSummarizer);

        let messages = vec![Message::new(Role::User, json!("A".repeat(100)))];

        let result = compactor.compact(messages.clone()).unwrap();

        // Should return unchanged
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_token_estimation() {
        let compactor = Compactor::new(100, MockSummarizer);

        // 400 characters = ~100 tokens (4 chars per token)
        let messages = vec![Message::new(Role::User, json!("A".repeat(400)))];

        let token_count = compactor.estimate_tokens(&messages);
        assert_eq!(token_count, 100);
    }

    #[test]
    fn test_empty_message_list() {
        let compactor = Compactor::new(100, MockSummarizer);

        let result = compactor.compact(vec![]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_odd_number_of_messages() {
        let compactor = Compactor::new(10, MockSummarizer);

        let messages = vec![
            Message::new(Role::User, json!("A".repeat(50))),
            Message::new(Role::Assistant, json!("B".repeat(50))),
            Message::new(Role::User, json!("C".repeat(50))),
        ];

        let result = compactor.compact(messages).unwrap();

        // 3 messages: split at 1, so 1 older + 2 newer
        assert_eq!(result.len(), 3); // summary + 2 newer
        assert!(result[0].content.to_string().contains("Summary of 1"));
    }

    #[test]
    fn test_compacted_metadata() {
        let compactor = Compactor::new(10, MockSummarizer);

        let messages = vec![
            Message::new(Role::User, json!("A".repeat(50))),
            Message::new(Role::Assistant, json!("B".repeat(50))),
            Message::new(Role::User, json!("C".repeat(50))),
            Message::new(Role::Assistant, json!("D".repeat(50))),
        ];

        let result = compactor.compact(messages).unwrap();

        let metadata = result[0].metadata.as_ref().unwrap();
        assert_eq!(metadata["compacted"], true);
        assert_eq!(metadata["original_count"], 2);
    }
}
