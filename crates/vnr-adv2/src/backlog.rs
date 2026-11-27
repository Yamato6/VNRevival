//! The message history (backlog).
//!
//! The original adds an entry (`0x405ae0`) for messages (`sys 01`), chat
//! lines (`sys a0`/`a2`) and the lines of `sys 04`, which are text that is
//! otherwise only in pictures or windows (the monologues, mails read
//! aloud). [`History`] keeps the last [`LIMIT`] of them for the
//! interface's history screen.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Entries kept.
pub const LIMIT: usize = 300;

/// One line of the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub speaker: Option<String>,
    pub text: String,
    /// Voice file to replay.
    pub voice: Option<String>,
}

/// The entries so far, oldest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    entries: VecDeque<Entry>,
}

impl History {
    pub fn push(&mut self, entry: Entry) {
        if entry.text.trim().is_empty() {
            return;
        }
        if self.entries.len() == LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> Entry {
        Entry {
            speaker: None,
            text: text.into(),
            voice: None,
        }
    }

    #[test]
    fn keeps_the_newest_entries() {
        let mut history = History::default();
        for i in 0..LIMIT + 5 {
            history.push(entry(&i.to_string()));
        }
        history.push(entry("  "));
        assert_eq!(history.len(), LIMIT);
        assert_eq!(history.entries().next().unwrap().text, "5");
    }
}
