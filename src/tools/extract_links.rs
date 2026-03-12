use anyhow::{anyhow, Result};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use tracing::{debug, info};

use super::{Tool, ToolContext};

struct BuiltinPattern {
    kind: &'static str,
    regex: &'static str,
    /// Index of the capture group that contains the actual link / path value.
    /// 0 means the whole match is the value (no sub-groups needed).
    link_group: usize,
}

const BUILTIN_PATTERNS: &[BuiltinPattern] = &[
    BuiltinPattern {
        kind: "include_file",
        regex: r#"\[include\s+file="([^"]+)"\]"#,
        link_group: 1,
    },
    BuiltinPattern {
        kind: "markdown_link",
        regex: r"\[([^\]]+)\]\(([^)]+)\)",
        link_group: 2,
    },
    BuiltinPattern {
        kind: "http_url",
        regex: r#"https?://[^\s"'<>)\]]+"#,
        link_group: 0,
    },
    BuiltinPattern {
        kind: "html_href",
        regex: r#"href="([^"]+)""#,
        link_group: 1,
    },
    BuiltinPattern {
        kind: "html_src",
        regex: r#"src="([^"]+)""#,
        link_group: 1,
    },
];

pub struct ExtractLinksTool;

#[async_trait]
impl Tool for ExtractLinksTool {
    fn name(&self) -> &str {
        "extract_links"
    }

    fn description(&self) -> &str {
        "Extract all links and file references from a document by scanning with every built-in \
         pattern at once. Recognised patterns: \
         [include file=\"...\"] directives, \
         Markdown [text](url) links (external/relative only, not same-page anchors), \
         bare HTTP/HTTPS URLs, \
         HTML href=\"...\" and src=\"...\" attributes. \
         Supply the content via 'filepath' (must be inside artifacts/), 'url', or inline 'text'. \
         Prefer filepath/url over inline text to avoid token-limit issues with large documents. \
         Returns a flat, deduplicated list of link values together with the pattern type that \
         found each one. Optionally accepts 'extra_regex' for a one-off custom pattern."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filepath": {
                    "type": "string",
                    "description": "Path to a local file inside the 'artifacts/' directory \
                                    (e.g. 'artifacts/doc.md'). Mutually exclusive with 'url' and 'text'."
                },
                "url": {
                    "type": "string",
                    "description": "HTTP/HTTPS URL to fetch and scan. \
                                    Mutually exclusive with 'filepath' and 'text'."
                },
                "text": {
                    "type": "string",
                    "description": "Inline text to scan. Use only for short snippets; \
                                    prefer 'filepath' or 'url' for large documents to avoid \
                                    truncation. Mutually exclusive with 'filepath' and 'url'."
                },
                "extra_regex": {
                    "type": "string",
                    "description": "Optional additional regular expression. \
                                    The first capture group is used as the link value; \
                                    if the pattern has no groups, the whole match is used."
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let filepath = params["filepath"].as_str();
        let url      = params["url"].as_str();
        let inline   = params["text"].as_str();

        let source_count = [filepath, url, inline].iter().filter(|v| v.is_some()).count();
        if source_count > 1 {
            return Err(anyhow!(
                "extract_links: provide exactly one of 'filepath', 'url', or 'text'"
            ));
        }

        let text: String = match (filepath, url, inline) {
            (Some(path), None, None) => {
                validate_artifacts_path(path)?;
                ctx.log(format!("extract_links: reading '{path}'")).await;
                std::fs::read_to_string(path)
                    .map_err(|e| anyhow!("extract_links: cannot read '{path}' – {e}"))?
            }
            (None, Some(remote), None) => {
                ctx.log(format!("extract_links: fetching {remote}")).await;
                let resp = ctx.http.get(remote).send().await?;
                if !resp.status().is_success() {
                    return Err(anyhow!("extract_links: HTTP {} for {remote}", resp.status()));
                }
                resp.text().await?
            }
            (None, None, Some(t)) => t.to_string(),
            (None, None, None) => {
                return Err(anyhow!(
                    "extract_links: one of 'filepath', 'url', or 'text' is required"
                ));
            }
            _ => unreachable!(),
        };

        let text = text.as_str();
        let extra_regex = params["extra_regex"].as_str();

        ctx.log("extract_links: scanning all built-in patterns".to_string()).await;

        // Collect (value, kind) pairs; use a vec to preserve order, deduplicate after.
        let mut found: Vec<(String, &str)> = Vec::new();

        for bp in BUILTIN_PATTERNS {
            let re = Regex::new(bp.regex)
                .map_err(|e| anyhow!("extract_links: bad built-in regex for '{}' – {e}", bp.kind))?;

            for caps in re.captures_iter(text) {
                let value = caps
                    .get(bp.link_group)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();

                if !value.is_empty() && is_actionable_link(&value) {
                    found.push((value, bp.kind));
                }
            }
        }

        if let Some(re_str) = extra_regex {
            ctx.log("extract_links: also scanning with extra_regex".to_string()).await;
            let re = Regex::new(re_str)
                .map_err(|e| anyhow!("extract_links: invalid extra_regex – {e}"))?;

            let use_group = re.captures_len() > 1;

            for caps in re.captures_iter(text) {
                let value = if use_group {
                    caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default()
                } else {
                    caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default()
                };

                if !value.is_empty() && is_actionable_link(&value) {
                    found.push((value, "custom"));
                }
            }
        }

        // Deduplicate while preserving first-seen order.
        let mut seen = std::collections::HashSet::new();
        let links: Vec<Value> = found
            .into_iter()
            .filter(|(v, _)| seen.insert(v.clone()))
            .map(|(value, kind)| json!({ "value": value, "type": kind }))
            .collect();

        let count = links.len();
        info!(count, "extract_links: done");
        debug!(links = ?links, "extract_links: found links");
        ctx.log(format!("extract_links: found {count} unique link(s)")).await;

        Ok(json!({
            "count": count,
            "links": links
        }))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn validate_artifacts_path(path: &str) -> Result<()> {
    let canonical = Path::new(path).components().collect::<std::path::PathBuf>();
    let first = canonical
        .components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .unwrap_or("");
    if first != "artifacts" {
        return Err(anyhow!(
            "extract_links: file access is restricted to the 'artifacts/' directory, got '{path}'"
        ));
    }
    if canonical
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "extract_links: path traversal is not allowed (got '{path}')"
        ));
    }
    Ok(())
}

/// Returns `true` for links that refer to an external resource or a file.
/// Filters out same-page anchors (`#section`) and empty/javascript pseudo-links.
fn is_actionable_link(value: &str) -> bool {
    let v = value.trim();
    !v.is_empty()
        && !v.starts_with('#')
        && !v.starts_with("javascript:")
        && v != "/"
}
