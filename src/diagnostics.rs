//! Diagnostics module for Log Scout Analyzer
//!
//! Handles error detection, warning generation, and diagnostic reporting
//! for pattern-matched log entries using LSP diagnostics protocol.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Diagnostic severity levels matching LSP specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// Position in a document (0-based line and character)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

impl Position {
    pub fn new(line: usize, character: usize) -> Self {
        Self { line, character }
    }
}

/// Range in a document (start and end positions)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn single_line(line: usize, start_char: usize, end_char: usize) -> Self {
        Self {
            start: Position::new(line, start_char),
            end: Position::new(line, end_char),
        }
    }

    pub fn multi_line(start_line: usize, start_char: usize, end_line: usize, end_char: usize) -> Self {
        Self {
            start: Position::new(start_line, start_char),
            end: Position::new(end_line, end_char),
        }
    }
}

/// Code action that can be applied to fix a diagnostic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAction {
    pub title: String,
    pub kind: String,
    pub edit: Option<WorkspaceEdit>,
}

/// Workspace edit for applying fixes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEdit {
    pub changes: HashMap<String, Vec<TextEdit>>,
}

/// Text edit to modify document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

/// A diagnostic entry representing an issue in the log file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: DiagnosticSeverity,
    pub code: Option<String>,
    pub source: String,
    pub message: String,
    pub related_information: Vec<DiagnosticRelatedInformation>,
    pub tags: Vec<String>,
}

impl Diagnostic {
    pub fn new(
        range: Range,
        severity: DiagnosticSeverity,
        message: String,
    ) -> Self {
        Self {
            range,
            severity,
            code: None,
            source: "log-scout".to_string(),
            message,
            related_information: Vec::new(),
            tags: Vec::new(),
        }
    }

    pub fn error(range: Range, message: String) -> Self {
        Self::new(range, DiagnosticSeverity::Error, message)
    }

    pub fn warning(range: Range, message: String) -> Self {
        Self::new(range, DiagnosticSeverity::Warning, message)
    }

    pub fn info(range: Range, message: String) -> Self {
        Self::new(range, DiagnosticSeverity::Information, message)
    }

    pub fn hint(range: Range, message: String) -> Self {
        Self::new(range, DiagnosticSeverity::Hint, message)
    }

    pub fn with_code(mut self, code: String) -> Self {
        self.code = Some(code);
        self
    }

    pub fn with_tag(mut self, tag: String) -> Self {
        self.tags.push(tag);
        self
    }

    pub fn with_related(mut self, related: DiagnosticRelatedInformation) -> Self {
        self.related_information.push(related);
        self
    }
}

/// Related diagnostic information (e.g., other occurrences of the same pattern)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticRelatedInformation {
    pub location: Location,
    pub message: String,
}

/// Location in a document (URI and range)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// Collection of diagnostics for a document
#[derive(Debug, Default)]
pub struct DiagnosticCollection {
    diagnostics: HashMap<String, Vec<Diagnostic>>,
}

impl DiagnosticCollection {
    pub fn new() -> Self {
        Self {
            diagnostics: HashMap::new(),
        }
    }

    pub fn add(&mut self, uri: String, diagnostic: Diagnostic) {
        self.diagnostics
            .entry(uri)
            .or_insert_with(Vec::new)
            .push(diagnostic);
    }

    pub fn add_multiple(&mut self, uri: String, diagnostics: Vec<Diagnostic>) {
        self.diagnostics
            .entry(uri)
            .or_insert_with(Vec::new)
            .extend(diagnostics);
    }

    pub fn clear(&mut self, uri: &str) {
        self.diagnostics.remove(uri);
    }

    pub fn clear_all(&mut self) {
        self.diagnostics.clear();
    }

    pub fn get(&self, uri: &str) -> Option<&Vec<Diagnostic>> {
        self.diagnostics.get(uri)
    }

    pub fn get_all(&self) -> &HashMap<String, Vec<Diagnostic>> {
        &self.diagnostics
    }

    pub fn count(&self, uri: &str) -> usize {
        self.diagnostics.get(uri).map_or(0, |d| d.len())
    }

    pub fn count_by_severity(&self, uri: &str, severity: DiagnosticSeverity) -> usize {
        self.diagnostics
            .get(uri)
            .map_or(0, |diagnostics| {
                diagnostics
                    .iter()
                    .filter(|d| d.severity == severity)
                    .count()
            })
    }

    pub fn total_count(&self) -> usize {
        self.diagnostics.values().map(|v| v.len()).sum()
    }
}

/// Builder for creating complex diagnostics
pub struct DiagnosticBuilder {
    diagnostic: Diagnostic,
}

impl DiagnosticBuilder {
    pub fn new(range: Range, severity: DiagnosticSeverity, message: String) -> Self {
        Self {
            diagnostic: Diagnostic::new(range, severity, message),
        }
    }

    pub fn error(range: Range, message: String) -> Self {
        Self::new(range, DiagnosticSeverity::Error, message)
    }

    pub fn warning(range: Range, message: String) -> Self {
        Self::new(range, DiagnosticSeverity::Warning, message)
    }

    pub fn info(range: Range, message: String) -> Self {
        Self::new(range, DiagnosticSeverity::Information, message)
    }

    pub fn hint(range: Range, message: String) -> Self {
        Self::new(range, DiagnosticSeverity::Hint, message)
    }

    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.diagnostic.code = Some(code.into());
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.diagnostic.source = source.into();
        self
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.diagnostic.tags.push(tag.into());
        self
    }

    pub fn related(mut self, location: Location, message: impl Into<String>) -> Self {
        self.diagnostic.related_information.push(DiagnosticRelatedInformation {
            location,
            message: message.into(),
        });
        self
    }

    pub fn build(self) -> Diagnostic {
        self.diagnostic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_creation() {
        let range = Range::single_line(10, 5, 20);
        let diag = Diagnostic::error(range, "Test error".to_string());

        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.message, "Test error");
        assert_eq!(diag.range.start.line, 10);
    }

    #[test]
    fn test_diagnostic_builder() {
        let range = Range::single_line(5, 0, 10);
        let diag = DiagnosticBuilder::warning(range, "Test warning".to_string())
            .code("W001")
            .tag("performance")
            .build();

        assert_eq!(diag.code, Some("W001".to_string()));
        assert_eq!(diag.tags.len(), 1);
        assert_eq!(diag.tags[0], "performance");
    }

    #[test]
    fn test_diagnostic_collection() {
        let mut collection = DiagnosticCollection::new();
        let uri = "file:///test.log".to_string();

        let range = Range::single_line(1, 0, 10);
        let diag1 = Diagnostic::error(range, "Error 1".to_string());
        let diag2 = Diagnostic::warning(range, "Warning 1".to_string());

        collection.add(uri.clone(), diag1);
        collection.add(uri.clone(), diag2);

        assert_eq!(collection.count(&uri), 2);
        assert_eq!(collection.count_by_severity(&uri, DiagnosticSeverity::Error), 1);
        assert_eq!(collection.count_by_severity(&uri, DiagnosticSeverity::Warning), 1);
    }

    #[test]
    fn test_position_and_range() {
        let pos1 = Position::new(5, 10);
        let pos2 = Position::new(5, 20);
        let range = Range::new(pos1, pos2);

        assert_eq!(range.start.line, 5);
        assert_eq!(range.start.character, 10);
        assert_eq!(range.end.character, 20);
    }

    #[test]
    fn test_multi_line_range() {
        let range = Range::multi_line(10, 5, 12, 15);

        assert_eq!(range.start.line, 10);
        assert_eq!(range.start.character, 5);
        assert_eq!(range.end.line, 12);
        assert_eq!(range.end.character, 15);
    }
}
