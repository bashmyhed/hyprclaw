use crate::types::*;

const MAX_RECENT_HISTORY: usize = 50;
const MAX_TOTAL_TOKENS: usize = 100_000;
const SUMMARY_THRESHOLD: usize = 30;
const TEMP_CONTEXT_RETENTION_SECS: i64 = 3 * 24 * 60 * 60;

pub struct ContextCompactor;

impl ContextCompactor {
    pub fn compact(context: &mut ContextData) -> bool {
        let mut compacted = false;

        if Self::compact_history(context) {
            compacted = true;
        }
        Self::deduplicate_facts(context);
        Self::prune_completed_tasks(context);

        compacted
    }

    fn compact_history(context: &mut ContextData) -> bool {
        let history = &mut context.recent_history;
        let mut compacted = false;

        // Time-based compaction: archive temporary context older than a few days.
        let cutoff = chrono::Utc::now().timestamp() - TEMP_CONTEXT_RETENTION_SECS;
        let stale_count = history
            .iter()
            .take_while(|entry| entry.timestamp > 0 && entry.timestamp < cutoff)
            .count();
        if stale_count > 0 {
            let stale_entries: Vec<_> = history.drain(..stale_count).collect();
            let summary = format!(
                "Archived temporary context (older than {} days):\n{}",
                TEMP_CONTEXT_RETENTION_SECS / 86_400,
                Self::summarize_entries(&stale_entries)
            );
            context.long_term_summary = if context.long_term_summary.is_empty() {
                summary
            } else {
                format!("{}\n\n{}", context.long_term_summary, summary)
            };
            tracing::info!(
                "Time-based compaction: archived {} old entries",
                stale_count
            );
            compacted = true;
        }

        // If history is too long, summarize older entries
        if history.len() > MAX_RECENT_HISTORY {
            let to_summarize = history.len() - SUMMARY_THRESHOLD;
            let old_entries: Vec<_> = history.drain(..to_summarize).collect();

            // Create summary
            let summary = Self::summarize_entries(&old_entries);
            context.long_term_summary = if context.long_term_summary.is_empty() {
                summary
            } else {
                format!("{}\n\n{}", context.long_term_summary, summary)
            };

            tracing::info!("Compacted {} history entries", to_summarize);
            compacted = true;
        }

        // Token-based compaction
        let total_tokens: usize = history.iter().filter_map(|e| e.token_count).sum();

        if total_tokens > MAX_TOTAL_TOKENS {
            let target = history.len() / 2;
            let removed: Vec<_> = history.drain(..target).collect();
            let summary = Self::summarize_entries(&removed);

            context.long_term_summary = if context.long_term_summary.is_empty() {
                summary
            } else {
                format!("{}\n\n{}", context.long_term_summary, summary)
            };

            tracing::info!("Token-based compaction: removed {} entries", target);
            compacted = true;
        }

        compacted
    }

    fn summarize_entries(entries: &[HistoryEntry]) -> String {
        let mut summary = String::from("Previous conversation summary:\n");

        for entry in entries {
            let preview = if entry.content.len() > 100 {
                format!("{}...", &entry.content[..100])
            } else {
                entry.content.clone()
            };
            summary.push_str(&format!("- {}: {}\n", entry.role, preview));
        }

        summary
    }

    fn deduplicate_facts(context: &mut ContextData) {
        let original_len = context.facts.len();
        context.facts.sort();
        context.facts.dedup();

        if context.facts.len() < original_len {
            tracing::debug!(
                "Deduplicated facts: {} -> {}",
                original_len,
                context.facts.len()
            );
        }
    }

    fn prune_completed_tasks(context: &mut ContextData) {
        let original_len = context.active_tasks.len();

        // Keep only non-completed tasks from last 24 hours
        let cutoff = chrono::Utc::now().timestamp() - 86400;
        context
            .active_tasks
            .retain(|task| task.status != "Completed" || task.updated_at > cutoff);

        if context.active_tasks.len() < original_len {
            tracing::debug!(
                "Pruned tasks: {} -> {}",
                original_len,
                context.active_tasks.len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_compaction() {
        let mut context = ContextData::default();

        // Add many entries
        for i in 0..100 {
            context.recent_history.push(HistoryEntry {
                timestamp: i,
                role: "user".to_string(),
                content: format!("Message {}", i),
                token_count: Some(10),
            });
        }

        let compacted = ContextCompactor::compact(&mut context);

        assert!(compacted);
        assert!(context.recent_history.len() <= MAX_RECENT_HISTORY);
        assert!(!context.long_term_summary.is_empty());
    }

    #[test]
    fn test_fact_deduplication() {
        let mut context = ContextData::default();
        context.facts = vec![
            "fact1".to_string(),
            "fact2".to_string(),
            "fact1".to_string(),
            "fact3".to_string(),
            "fact2".to_string(),
        ];

        ContextCompactor::compact(&mut context);

        assert_eq!(context.facts.len(), 3);
    }
}
