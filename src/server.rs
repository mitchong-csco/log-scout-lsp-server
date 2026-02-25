//! LSP Server Implementation
//!
//! Implements the Language Server Protocol for log file analysis.

use crate::pattern_engine::{Detection, PatternEngine, Severity};
use crate::tagscout::{SyncMode, SyncService, SyncServiceConfig};

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

/// Main LSP server structure
#[derive(Clone)]
pub struct LogScoutServer {
    client: Client,
    pattern_engine: Arc<RwLock<Option<PatternEngine>>>,
    tagscout_service: Arc<RwLock<Option<SyncService>>>,
    documents: Arc<DashMap<Url, String>>,
}

impl LogScoutServer {
    /// Create a new LSP server instance
    pub fn new(client: Client) -> Self {
        // No default patterns - rely entirely on TagScout
        let pattern_engine = Self::load_default_patterns();

        if pattern_engine.is_some() {
            tracing::info!("Pattern engine initialized with default patterns");
        } else {
            tracing::info!("Pattern engine will be initialized after TagScout patterns load");
        }

        Self {
            client,
            pattern_engine: Arc::new(RwLock::new(pattern_engine)),
            tagscout_service: Arc::new(RwLock::new(None)),
            documents: Arc::new(DashMap::new()),
        }
    }

    /// Initialize TagScout integration
    pub async fn initialize_tagscout(&self) -> std::result::Result<(), String> {
        tracing::info!("Initializing TagScout integration");

        // Configure sync service with cache-first mode for offline support
        let mut config = SyncServiceConfig::default();
        config.sync_mode = SyncMode::CacheFirst; // Try cache first, fallback to MongoDB
        config.cache_ttl_seconds = 3600; // 1 hour cache
        config.auto_refresh_interval = Some(300); // Auto-refresh every 5 minutes

        let mut service = SyncService::new(config)
            .await
            .map_err(|e| format!("Failed to initialize TagScout: {}", e))?;

        // Perform initial sync (from cache or MongoDB)
        let result = service
            .initialize()
            .await
            .map_err(|e| format!("TagScout sync failed: {}", e))?;

        tracing::info!(
            "TagScout initialized: {} patterns loaded {} (age: {}ms)",
            result.patterns_fetched,
            if result.from_cache {
                "from cache"
            } else {
                "from MongoDB"
            },
            result.duration_ms
        );

        // Load patterns into engine
        let patterns = service
            .get_patterns()
            .await
            .map_err(|e| format!("Failed to get patterns: {}", e))?;

        if !patterns.is_empty() {
            let engine = PatternEngine::new(patterns, 0.7, 10)
                .map_err(|e| format!("Failed to create pattern engine: {}", e))?;

            *self.pattern_engine.write().await = Some(engine);
            tracing::info!("Pattern engine updated with TagScout patterns");
        }

        // Store service
        *self.tagscout_service.write().await = Some(service);
        tracing::info!("TagScout service stored successfully");

        Ok(())
    }

    /// Refresh patterns from TagScout
    pub async fn refresh_tagscout_patterns(&self) -> std::result::Result<usize, String> {
        // Clone the Arc to avoid holding the lock across await
        let service_opt = {
            let guard = self.tagscout_service.read().await;
            guard.as_ref().map(|_| ())
        };

        if service_opt.is_some() {
            let guard = self.tagscout_service.read().await;
            let service = guard.as_ref().unwrap();

            let result = service
                .sync()
                .await
                .map_err(|e| format!("Sync failed: {}", e))?;

            tracing::info!(
                "Refreshed {} patterns from TagScout",
                result.patterns_fetched
            );

            // Update pattern engine
            let patterns = service
                .get_patterns()
                .await
                .map_err(|e| format!("Failed to get patterns: {}", e))?;

            let count = patterns.len();
            if !patterns.is_empty() {
                let engine = PatternEngine::new(patterns, 0.7, 10)
                    .map_err(|e| format!("Failed to update engine: {}", e))?;
                *self.pattern_engine.write().await = Some(engine);
                Ok(count)
            } else {
                Ok(0)
            }
        } else {
            Err("TagScout service not initialized".to_string())
        }
    }

    /// Load default pattern set (fallback when TagScout unavailable)
    fn load_default_patterns() -> Option<PatternEngine> {
        // No default patterns - rely entirely on TagScout for meaningful categorization
        // Return None to ensure no pattern engine is initialized until TagScout loads
        None
    }

    /// Analyze text and return diagnostics (shared by push and pull)
    async fn analyze_text(&self, text: &str, _uri: &str, total_lines: usize) -> Vec<Diagnostic> {
        let engine_guard = self.pattern_engine.read().await;
        if let Some(engine) = engine_guard.as_ref() {
            let mut all_detections = Vec::new();
            let mut processed = 0;

            // STAGE 1: Pattern Matching - Analyze each line
            for (line_num, line) in text.lines().enumerate() {
                let detections = engine.process_line(line, line_num);
                all_detections.extend(detections);
                processed += 1;

                // Report progress every 1000 lines
                if processed % 1000 == 0 {
                    let percentage = (processed as f64 / total_lines as f64 * 100.0) as u32;
                    self.client
                        .log_message(
                            MessageType::LOG,
                            &format!(
                                "Analyzing: {}% ({}/{} lines)",
                                percentage, processed, total_lines
                            ),
                        )
                        .await;
                }
            }

            tracing::info!(
                "Found {} detections (before deduplication)",
                all_detections.len()
            );

            // TODO: STAGE 2: Signature Detection - Group patterns in same category
            // let signatures = signature_engine.detect(&all_detections);

            // TODO: STAGE 3: Process Correlation - Identify functional flows
            // let processes = process_engine.correlate(&signatures);

            // TODO: STAGE 4: Scenario Analysis - Cross-category event correlation
            // let scenarios = scenario_engine.analyze(&processes);

            // STAGE 5: Deduplication - Remove overlapping pattern matches
            all_detections = Self::deduplicate_detections(all_detections);

            tracing::info!(
                "Found {} unique detections (after deduplication)",
                all_detections.len()
            );

            // TODO: STAGE 6: Remediation - Generate action plans for deduplicated issues
            // let remediations = remediation_engine.recommend(&all_detections, &signatures, &scenarios);

            // STAGE 7: Diagnostic Creation - Convert to LSP diagnostics
            all_detections
                .into_iter()
                .map(|detection| self.detection_to_diagnostic(&detection))
                .collect()
        } else {
            tracing::warn!("No pattern engine available");
            vec![]
        }
    }

    /// Deduplicate detections that overlap on the same line
    ///
    /// When multiple patterns match the same location (line + column range),
    /// keep only the one with the highest severity. This handles cases where
    /// TagScout has multiple patterns with the same regex but different templates
    /// (e.g., HTTP success vs error patterns that both match any HTTP response).
    ///
    /// Future stages (signatures, scenarios) will see ALL matches before deduplication,
    /// so they have full context for analysis.
    fn deduplicate_detections(
        detections: Vec<crate::pattern_engine::Detection>,
    ) -> Vec<crate::pattern_engine::Detection> {
        use std::collections::HashMap;

        // Group detections by line and column range
        let mut grouped: HashMap<(usize, (usize, usize)), Vec<crate::pattern_engine::Detection>> =
            HashMap::new();

        for detection in detections {
            let key = (detection.line_number, detection.column_range);
            grouped.entry(key).or_insert_with(Vec::new).push(detection);
        }

        // For each group, keep only the highest severity
        let mut deduplicated = Vec::new();
        for (_, mut group) in grouped {
            if group.len() == 1 {
                deduplicated.push(group.pop().unwrap());
            } else {
                // Sort by severity (Error > Warning > Info > Hint)
                group.sort_by_key(|d| match d.final_severity {
                    crate::pattern_engine::Severity::Error => 0,
                    crate::pattern_engine::Severity::Warning => 1,
                    crate::pattern_engine::Severity::Info => 2,
                    crate::pattern_engine::Severity::Hint => 3,
                });

                // Keep the highest severity (first after sorting)
                if let Some(highest) = group.into_iter().next() {
                    deduplicated.push(highest);
                }
            }
        }

        // Sort by line number to maintain document order
        deduplicated.sort_by_key(|d| d.line_number);

        deduplicated
    }

    /// Analyze document and publish diagnostics (push mode)
    async fn analyze_and_publish(&self, uri: &Url, text: &str) {
        tracing::debug!("Analyzing document (push): {}", uri);

        // Send status notification
        self.client
            .log_message(MessageType::INFO, &format!("🔍 Analyzing {}", uri.path()))
            .await;

        let total_lines = text.lines().count();
        let diagnostics = self.analyze_text(text, uri.as_str(), total_lines).await;

        // Publish diagnostics to client
        let count = diagnostics.len();
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;

        self.client
            .log_message(
                MessageType::INFO,
                &format!("✅ Analysis complete: {} issues found", count),
            )
            .await;
    }

    /// Replace template placeholders like {{ fieldName }} with actual values from field_values
    /// Handles all spacing variations: {{CODE}}, {{ CODE }}, {{ CODE}}, {{CODE }}
    fn substitute_template(
        template: &str,
        field_values: &std::collections::HashMap<String, String>,
    ) -> String {
        use regex::Regex;

        tracing::info!("=== SUBSTITUTE_TEMPLATE ===");
        tracing::info!("  Input template: '{}'", template);
        tracing::info!("  Field values: {:?}", field_values);

        let mut result = template.to_string();

        // Replace each placeholder with the actual value
        for (field_name, field_value) in field_values {
            // Create regex to match {{optional_spaces field_name optional_spaces}}
            // This handles: {{CODE}}, {{ CODE }}, {{ CODE}}, {{CODE }}, etc.
            let pattern_str = format!(r"\{{\{{\s*{}\s*\}}\}}", regex::escape(field_name));
            if let Ok(re) = Regex::new(&pattern_str) {
                let before = result.clone();
                result = re.replace_all(&result, field_value.as_str()).to_string();
                if before != result {
                    tracing::info!("  Replaced {{{{ {} }}}} with '{}'", field_name, field_value);
                }
            }
        }

        // Keep unsubstituted placeholders as-is (shows {{ FIELD }} instead of [...])
        // This makes it clear which fields weren't extracted

        tracing::info!("  Output result: '{}'", result);
        tracing::info!("=== END SUBSTITUTE_TEMPLATE ===");

        result
    }

    /// Convert a Detection to an LSP Diagnostic
    fn detection_to_diagnostic(&self, detection: &Detection) -> Diagnostic {
        let severity = match detection.pattern.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Info => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
        };

        let (start_col, end_col) = detection.column_range;

        // Substitute template placeholders in category
        let category =
            Self::substitute_template(&detection.pattern.category, &detection.field_values);

        // Get raw template from pattern (the message template from TagScout)
        let template = if detection.pattern.annotation.is_empty() {
            "(missing)".to_string()
        } else {
            detection.pattern.annotation.clone()
        };

        tracing::info!("=== COMPUTING MERGED_TEMPLATE ===");
        tracing::info!("  Pattern name: {}", detection.pattern.name);
        tracing::info!("  Template: '{}'", template);
        tracing::info!("  Field values count: {}", detection.field_values.len());

        // Create merged template - template with substituted values
        let merged_template = if template == "(missing)" {
            tracing::error!(
                "  Template is MISSING for pattern '{}' - this pattern requires an annotation",
                detection.pattern.id
            );
            template.clone() // Diagnostic message will be "(missing)"
        } else {
            // Substitute field values into template
            let substituted = Self::substitute_template(&template, &detection.field_values);
            tracing::info!(
                "  Successfully computed merged_template from template: '{}'",
                substituted
            );
            substituted
        };

        tracing::info!("  Final merged_template: '{}'", merged_template);
        tracing::info!("=== END COMPUTING MERGED_TEMPLATE ===");

        // Build diagnostic data with complete information
        let mut data_map = serde_json::Map::new();

        // Include ALL original TagScout annotation fields if available
        if let Some(ref tagscout_metadata) = detection.pattern.tagscout_metadata {
            if let serde_json::Value::Object(metadata_map) = tagscout_metadata {
                // Copy all original TagScout fields
                for (key, value) in metadata_map {
                    data_map.insert(key.clone(), value.clone());
                }
            }
        }

        // Build extracted_parameters as list of key-value pairs
        let extracted_params: Vec<serde_json::Value> = detection
            .field_values
            .iter()
            .map(|(name, value)| {
                let mut param = serde_json::Map::new();
                param.insert("name".to_string(), serde_json::Value::String(name.clone()));
                param.insert(
                    "value".to_string(),
                    serde_json::Value::String(value.clone()),
                );
                serde_json::Value::Object(param)
            })
            .collect();

        // Add/override with detection-specific fields using TagScout naming conventions
        tracing::info!("=== BUILDING DIAGNOSTIC DATA ===");
        tracing::info!("  template: '{}'", template);
        tracing::info!("  merged_template: '{}'", merged_template);
        tracing::info!("  log_line (matched): '{}'", detection.matched_text);

        // Core fields: template and merged result
        data_map.insert(
            "template".to_string(),
            serde_json::Value::String(template.clone()),
        );
        data_map.insert(
            "merged_template".to_string(),
            serde_json::Value::String(merged_template.clone()),
        );

        // Source information
        data_map.insert(
            "log_line".to_string(),
            serde_json::Value::String(detection.context.first().cloned().unwrap_or_default()),
        );

        // Extracted parameters as list of {name, value} objects
        data_map.insert(
            "extracted_parameters".to_string(),
            serde_json::Value::Array(extracted_params),
        );

        // Pattern metadata
        data_map.insert(
            "pattern_id".to_string(),
            serde_json::Value::String(detection.pattern.id.clone()),
        );
        data_map.insert(
            "pattern_name".to_string(),
            serde_json::Value::String(detection.pattern.name.clone()),
        );
        data_map.insert(
            "category".to_string(),
            serde_json::Value::String(category.clone()),
        );

        // Debugging information
        data_map.insert(
            "matched_text".to_string(),
            serde_json::Value::String(detection.matched_text.clone()),
        );
        data_map.insert(
            "pattern_regex".to_string(),
            serde_json::Value::String(detection.pattern.pattern.clone()),
        );

        // Include timestamp if present
        if let Some(ref timestamp) = detection.timestamp {
            data_map.insert(
                "timestamp".to_string(),
                serde_json::Value::String(timestamp.clone()),
            );
        }

        // Include log level if present
        if let Some(ref log_level) = detection.log_level {
            data_map.insert(
                "log_level".to_string(),
                serde_json::Value::String(format!("{:?}", log_level)),
            );
        }

        tracing::info!("  Data map has {} keys", data_map.len());
        tracing::info!("=== END BUILDING DIAGNOSTIC DATA ===");

        Diagnostic {
            range: Range {
                start: Position {
                    line: detection.line_number as u32,
                    character: start_col as u32,
                },
                end: Position {
                    line: detection.line_number as u32,
                    character: end_col as u32,
                },
            },
            severity: Some(severity),
            code: Some(NumberOrString::String(detection.pattern.id.clone())),
            code_description: None,
            source: Some("log-scout".to_string()),
            message: merged_template, // Main message is the merged template (substituted values)
            related_information: None,
            tags: None,
            data: Some(serde_json::Value::Object(data_map)),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for LogScoutServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("Client initializing LSP server");

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        identifier: Some("log-scout".to_string()),
                        inter_file_dependencies: false,
                        workspace_diagnostics: false,
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: Some(true),
                        },
                    },
                )),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "logScout.analyze".to_string(),
                        "logScout.showTimeline".to_string(),
                        "logScout.exportResults".to_string(),
                        "logScout.refreshPatterns".to_string(),
                        "logScout.getPatterns".to_string(),
                    ],
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: Some(true),
                    },
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "Log Scout Analyzer".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        tracing::info!("LSP server initialized successfully");
        self.client
            .log_message(MessageType::INFO, "Log Scout Analyzer ready!")
            .await;

        // Initialize TagScout in background
        let client_clone = self.client.clone();
        let server_clone = self.clone();
        tokio::spawn(async move {
            match server_clone.initialize_tagscout().await {
                Ok(_) => {
                    client_clone
                        .log_message(MessageType::INFO, "TagScout patterns loaded successfully")
                        .await;
                }
                Err(e) => {
                    client_clone
                        .log_message(
                            MessageType::WARNING,
                            &format!(
                                "TagScout initialization failed: {}. No patterns available.",
                                e
                            ),
                        )
                        .await;
                }
            }
        });
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("LSP server shutting down");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        tracing::info!("Document opened: {}", uri);

        // Store document
        self.documents.insert(uri.clone(), text.clone());

        // Analyze and publish diagnostics
        self.analyze_and_publish(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        tracing::debug!("Document changed: {}", uri);

        // Apply incremental changes
        if let Some(mut doc_entry) = self.documents.get_mut(&uri) {
            for change in params.content_changes {
                // For simplicity, just replace the whole document
                // In production, you'd handle incremental edits properly
                *doc_entry = change.text;
            }

            let text = doc_entry.clone();
            drop(doc_entry);

            // Re-analyze
            self.analyze_and_publish(&uri, &text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        tracing::info!("Document saved: {}", params.text_document.uri);

        // Re-analyze on save if text is provided
        if let Some(text) = params.text {
            self.documents
                .insert(params.text_document.uri.clone(), text.clone());
            self.analyze_and_publish(&params.text_document.uri, &text)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        tracing::info!("Document closed: {}", uri);

        // Remove from cache
        self.documents.remove(&uri);

        // Clear diagnostics
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(uri) {
            let lines: Vec<&str> = doc.lines().collect();
            if let Some(line) = lines.get(position.line as usize) {
                // Provide hover information about the line
                let contents = HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!(
                        "**Log Line Analysis**\n\nLine {}: `{}`\n\nLength: {} characters",
                        position.line + 1,
                        line,
                        line.len()
                    ),
                });

                return Ok(Some(Hover {
                    contents,
                    range: Some(Range {
                        start: Position {
                            line: position.line,
                            character: 0,
                        },
                        end: Position {
                            line: position.line,
                            character: line.len() as u32,
                        },
                    }),
                }));
            }
        }

        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;

        let mut actions = vec![];

        // "Export results" action
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Export analysis results".to_string(),
            kind: Some(CodeActionKind::REFACTOR),
            diagnostics: Some(params.context.diagnostics.clone()),
            edit: None,
            command: Some(Command {
                title: "Export Results".to_string(),
                command: "logScout.exportResults".to_string(),
                arguments: Some(vec![serde_json::to_value(uri).unwrap()]),
            }),
            is_preferred: Some(false),
            disabled: None,
            data: None,
        }));

        // "Show timeline" action
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Show timeline visualization".to_string(),
            kind: Some(CodeActionKind::EMPTY),
            diagnostics: None,
            edit: None,
            command: Some(Command {
                title: "Show Timeline".to_string(),
                command: "logScout.showTimeline".to_string(),
                arguments: Some(vec![serde_json::to_value(uri).unwrap()]),
            }),
            is_preferred: Some(false),
            disabled: None,
            data: None,
        }));

        Ok(Some(actions))
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        tracing::info!("Executing command: {}", params.command);

        match params.command.as_str() {
            "logScout.analyze" => {
                self.client
                    .log_message(MessageType::INFO, "Running full analysis...")
                    .await;
                Ok(None)
            }
            "logScout.showTimeline" => {
                self.client
                    .show_message(MessageType::INFO, "Opening timeline visualization...")
                    .await;
                Ok(None)
            }
            "logScout.exportResults" => {
                self.client
                    .show_message(MessageType::INFO, "Exporting analysis results...")
                    .await;
                Ok(None)
            }
            "logScout.refreshPatterns" => {
                self.client
                    .log_message(MessageType::INFO, "Refreshing patterns from TagScout...")
                    .await;

                match self.refresh_tagscout_patterns().await {
                    Ok(count) => {
                        self.client
                            .show_message(
                                MessageType::INFO,
                                &format!("Refreshed {} patterns", count),
                            )
                            .await;
                    }
                    Err(e) => {
                        self.client
                            .show_message(
                                MessageType::ERROR,
                                &format!("Failed to refresh patterns: {}", e),
                            )
                            .await;
                    }
                }
                Ok(None)
            }
            "logScout.getPatterns" => {
                tracing::info!("TagScout UI requesting patterns from LSP");

                let engine_guard = self.pattern_engine.read().await;
                if let Some(engine) = engine_guard.as_ref() {
                    let patterns = engine.get_patterns();
                    tracing::info!("Returning {} patterns to TagScout UI", patterns.len());

                    // Determine source based on pattern count and content
                    let source = if patterns.len() <= 6 {
                        "fallback_defaults"
                    } else {
                        "tagscout_mongodb"
                    };

                    // Convert patterns to JSON for the UI
                    let pattern_data: Vec<serde_json::Value> = patterns
                        .iter()
                        .map(|compiled| {
                            let p = &compiled.pattern;
                            serde_json::json!({
                                "id": p.id,
                                "name": p.name,
                                "description": p.annotation,
                                "pattern": p.pattern,
                                "severity": match p.severity {
                                    Severity::Error => "error",
                                    Severity::Warning => "warning",
                                    Severity::Info => "info",
                                    Severity::Hint => "hint",
                                },
                                "category": p.category,
                                "service": p.service,
                                "tags": p.tags,
                                "action": p.action,
                                "captureFields": p.capture_fields,
                                "parameterExtractors": p.parameter_extractors.iter().map(|pe| {
                                    serde_json::json!({
                                        "name": pe.name,
                                        "regex": pe.regex
                                    })
                                }).collect::<Vec<_>>(),
                            })
                        })
                        .collect();

                    Ok(Some(serde_json::json!({
                        "patterns": pattern_data,
                        "count": pattern_data.len(),
                        "source": source
                    })))
                } else {
                    tracing::warn!("No pattern engine available");
                    Ok(Some(serde_json::json!({
                        "patterns": [],
                        "count": 0,
                        "source": "none",
                        "error": "Pattern engine not initialized"
                    })))
                }
            }
            _ => {
                tracing::warn!("Unknown command: {}", params.command);
                Ok(None)
            }
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        if let Some(doc) = self.documents.get(uri) {
            let mut symbols = vec![];

            // Extract timeline events as symbols
            for (line_num, line) in doc.lines().enumerate() {
                // Simple heuristic: lines with timestamps or specific keywords
                if line.contains("ERROR")
                    || line.contains("WARNING")
                    || line.contains("INFO")
                    || line.contains("FATAL")
                {
                    #[allow(deprecated)]
                    let symbol = DocumentSymbol {
                        name: line.chars().take(50).collect::<String>(),
                        detail: Some(format!("Line {}", line_num + 1)),
                        kind: SymbolKind::EVENT,
                        tags: None,
                        deprecated: None,
                        range: Range {
                            start: Position {
                                line: line_num as u32,
                                character: 0,
                            },
                            end: Position {
                                line: line_num as u32,
                                character: line.len() as u32,
                            },
                        },
                        selection_range: Range {
                            start: Position {
                                line: line_num as u32,
                                character: 0,
                            },
                            end: Position {
                                line: line_num as u32,
                                character: line.len() as u32,
                            },
                        },
                        children: None,
                    };
                    symbols.push(symbol);
                }
            }

            return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
        }

        Ok(None)
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let uri = params.text_document.uri.clone();

        tracing::info!("Pull diagnostic request for: {}", uri);

        // Get document and analyze if we have it
        if let Some(doc) = self.documents.get(&uri) {
            let text = doc.clone();
            drop(doc);

            // Send status notification
            self.client
                .log_message(
                    MessageType::INFO,
                    &format!("🔍 Pull diagnostic request: {}", uri.path()),
                )
                .await;

            // Analyze the document
            let total_lines = text.lines().count();
            let diagnostics = self.analyze_text(&text, uri.as_str(), total_lines).await;

            tracing::info!(
                "Returning {} diagnostics for pull request",
                diagnostics.len()
            );

            self.client
                .log_message(
                    MessageType::INFO,
                    &format!(
                        "✅ Pull diagnostic complete: {} issues found",
                        diagnostics.len()
                    ),
                )
                .await;

            return Ok(DocumentDiagnosticReportResult::Report(
                DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items: diagnostics,
                    },
                }),
            ));
        }

        // No document found, return empty diagnostics
        tracing::info!("No document found for pull request: {}", uri);
        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items: vec![],
                },
            }),
        ))
    }
}
