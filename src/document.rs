//! Document Store
//!
//! Manages open documents and their state in the LSP server.

use dashmap::DashMap;
use tower_lsp::lsp_types::Url;

/// Document store for managing open files
pub struct DocumentStore {
    documents: DashMap<Url, Document>,
}

/// Represents a single document
pub struct Document {
    /// Full text content
    pub text: String,
    /// Version number (for incremental updates)
    pub version: i32,
    /// Language identifier
    pub language_id: String,
}

impl DocumentStore {
    /// Create a new document store
    pub fn new() -> Self {
        Self {
            documents: DashMap::new(),
        }
    }

    /// Open a new document
    pub fn open(&self, uri: Url, text: String, version: i32, language_id: String) {
        let document = Document {
            text,
            version,
            language_id,
        };
        self.documents.insert(uri, document);
    }

    /// Update an existing document
    pub fn update(&self, uri: &Url, text: String, version: i32) -> bool {
        if let Some(mut doc) = self.documents.get_mut(uri) {
            doc.text = text;
            doc.version = version;
            true
        } else {
            false
        }
    }

    /// Get document text
    pub fn get_text(&self, uri: &Url) -> Option<String> {
        self.documents.get(uri).map(|doc| doc.text.clone())
    }

    /// Get document
    pub fn get(&self, uri: &Url) -> Option<Document> {
        self.documents.get(uri).map(|doc| Document {
            text: doc.text.clone(),
            version: doc.version,
            language_id: doc.language_id.clone(),
        })
    }

    /// Close a document
    pub fn close(&self, uri: &Url) -> bool {
        self.documents.remove(uri).is_some()
    }

    /// Check if document exists
    pub fn contains(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }

    /// Get all document URIs
    pub fn uris(&self) -> Vec<Url> {
        self.documents
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get document count
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Get line at position
    pub fn get_line(&self, line: usize) -> Option<&str> {
        self.text.lines().nth(line)
    }

    /// Get line count
    pub fn line_count(&self) -> usize {
        self.text.lines().count()
    }

    /// Get text in range
    pub fn get_range(&self, start_line: usize, end_line: usize) -> String {
        self.text
            .lines()
            .skip(start_line)
            .take(end_line.saturating_sub(start_line) + 1)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_store() {
        let store = DocumentStore::new();
        let uri = Url::parse("file:///test.log").unwrap();

        // Open document
        store.open(
            uri.clone(),
            "line 1\nline 2".to_string(),
            1,
            "log".to_string(),
        );
        assert!(store.contains(&uri));
        assert_eq!(store.len(), 1);

        // Get text
        assert_eq!(store.get_text(&uri), Some("line 1\nline 2".to_string()));

        // Update
        assert!(store.update(&uri, "line 1\nline 2\nline 3".to_string(), 2));

        // Close
        assert!(store.close(&uri));
        assert!(!store.contains(&uri));
        assert!(store.is_empty());
    }

    #[test]
    fn test_document_lines() {
        let doc = Document {
            text: "line 1\nline 2\nline 3".to_string(),
            version: 1,
            language_id: "log".to_string(),
        };

        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.get_line(0), Some("line 1"));
        assert_eq!(doc.get_line(1), Some("line 2"));
        assert_eq!(doc.get_line(2), Some("line 3"));
        assert_eq!(doc.get_line(3), None);
    }
}
