use anyhow::{anyhow, Result};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use tracing::info;

use super::{Tool, ToolContext};

pub struct ReadLogTool;

#[async_trait]
impl Tool for ReadLogTool {
    fn name(&self) -> &str {
        "read_log"
    }

    fn description(&self) -> &str {
        "Read and filter lines from a log file. Supports filtering by log level \
         (WARN, ERRO/ERROR, CRIT/CRITICAL), searching by regex pattern, and reading \
         a specific line range. All filters are combined with AND logic. \
         Returns an array of matched lines with their original line numbers. \
         File path must start with 'artifacts/'."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filepath": {
                    "type": "string",
                    "description": "Path to the log file. Must start with 'artifacts/' \
                                    (e.g. 'artifacts/app.log')."
                },
                "levels": {
                    "type": "array",
                    "description": "Filter lines by log level. Accepted values: \
                                    'WARN', 'ERRO', 'CRIT'. Case-insensitive. \
                                    Lines containing any of the specified levels are included. \
                                    If omitted, all levels are included.",
                    "items": {
                        "type": "string",
                        "enum": ["WARN", "ERRO", "CRIT"]
                    }
                },
                "pattern": {
                    "type": "string",
                    "description": "Optional regex pattern to search within log lines. \
                                    Only lines matching this pattern are returned."
                },
                "from_line": {
                    "type": "integer",
                    "description": "1-based start line number (inclusive). \
                                    If omitted, starts from the first line.",
                    "minimum": 1
                },
                "to_line": {
                    "type": "integer",
                    "description": "1-based end line number (inclusive). \
                                    If omitted, reads to the end of the file.",
                    "minimum": 1
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matching lines to return. \
                                    Defaults to 500 to avoid overwhelming the context window.",
                    "minimum": 1,
                    "maximum": 5000
                }
            },
            "required": ["filepath"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let filepath = params["filepath"]
            .as_str()
            .ok_or_else(|| anyhow!("read_log: missing 'filepath' parameter"))?;

        validate_artifacts_path(filepath)?;

        let from_line = params["from_line"].as_u64().map(|n| n as usize);
        let to_line = params["to_line"].as_u64().map(|n| n as usize);
        let max_results = params["max_results"].as_u64().unwrap_or(500) as usize;

        if let (Some(from), Some(to)) = (from_line, to_line) {
            if from > to {
                return Err(anyhow!(
                    "read_log: 'from_line' ({from}) must be <= 'to_line' ({to})"
                ));
            }
        }

        // Build level keywords from the levels array
        let level_keywords: Vec<String> = if let Some(arr) = params["levels"].as_array() {
            arr.iter()
                .filter_map(|v| v.as_str())
                .flat_map(|level| level_variants(level))
                .collect()
        } else {
            vec![]
        };

        // Compile optional regex pattern
        let pattern: Option<Regex> = if let Some(pat) = params["pattern"].as_str() {
            Some(
                Regex::new(pat)
                    .map_err(|e| anyhow!("read_log: invalid regex pattern '{pat}': {e}"))?,
            )
        } else {
            None
        };

        ctx.log(format!("read_log: opening '{filepath}'")).await;

        let file = File::open(filepath)
            .map_err(|e| anyhow!("read_log: cannot open '{filepath}': {e}"))?;

        let reader = BufReader::new(file);
        let mut matched: Vec<Value> = Vec::new();
        let mut total_lines = 0usize;
        let mut scanned_lines = 0usize;

        for (idx, line_result) in reader.lines().enumerate() {
            let line_number = idx + 1; // 1-based
            total_lines += 1;

            // Apply line range filter
            if let Some(from) = from_line {
                if line_number < from {
                    continue;
                }
            }
            if let Some(to) = to_line {
                if line_number > to {
                    break;
                }
            }

            scanned_lines += 1;

            let line = line_result
                .map_err(|e| anyhow!("read_log: error reading line {line_number}: {e}"))?;

            // Apply level filter (OR across all requested levels)
            if !level_keywords.is_empty() {
                let upper = line.to_uppercase();
                let matches_level = level_keywords.iter().any(|kw| upper.contains(kw.as_str()));
                if !matches_level {
                    continue;
                }
            }

            // Apply regex pattern filter
            if let Some(ref re) = pattern {
                if !re.is_match(&line) {
                    continue;
                }
            }

            matched.push(json!({
                "line_number": line_number,
                "content": line
            }));

            if matched.len() >= max_results {
                break;
            }
        }

        info!(
            filepath = %filepath,
            total_lines = total_lines,
            scanned = scanned_lines,
            matched = matched.len(),
            "read_log: done"
        );
        ctx.log(format!(
            "read_log: scanned {scanned_lines} lines, {} matched",
            matched.len()
        ))
        .await;

        Ok(json!({
            "filepath": filepath,
            "total_lines_scanned": scanned_lines,
            "matched_count": matched.len(),
            "truncated": matched.len() >= max_results,
            "lines": matched
        }))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns all textual variants that should be recognised for a given level keyword.
fn level_variants(level: &str) -> Vec<String> {
    match level.to_uppercase().as_str() {
        "WARN" => vec!["WARN".into()],
        "ERROR" => vec!["ERROR".into(), "ERRO".into()],
        "CRIT" => vec!["CRIT".into(), "CRITICAL".into()],
        other => vec![other.to_uppercase()],
    }
}

fn validate_artifacts_path(path: &str) -> Result<()> {
    use std::path::{Component, Path};

    let canonical = Path::new(path).components().collect::<std::path::PathBuf>();

    let first = canonical
        .components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .unwrap_or("");

    if first != "artifacts" {
        return Err(anyhow!(
            "read_log: file access is restricted to the 'artifacts/' directory, got '{path}'"
        ));
    }

    if canonical
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(anyhow!(
            "read_log: path traversal is not allowed (got '{path}')"
        ));
    }

    Ok(())
}
