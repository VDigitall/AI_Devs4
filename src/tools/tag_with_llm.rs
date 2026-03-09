use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use super::{Tool, ToolContext};

pub struct TagWithLlmTool;

#[async_trait]
impl Tool for TagWithLlmTool {
    fn name(&self) -> &str {
        "tag_with_llm"
    }

    fn description(&self) -> &str {
        "Tag each item in a JSON array using an LLM. For each item, the LLM assigns zero or more \
         tags from the provided tag list based on the value of a specified field. \
         Returns the original array with a 'tags' field added to each object."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "data": {
                    "type": "array",
                    "description": "Array of objects to tag",
                    "items": { "type": "object" }
                },
                "field": {
                    "type": "string",
                    "description": "The field in each object whose value is sent to the LLM for tagging"
                },
                "tags": {
                    "type": "array",
                    "description": "The exhaustive list of allowed tags the LLM can assign",
                    "items": { "type": "string" }
                },
                "instructions": {
                    "type": "string",
                    "description": "Optional extra instructions for the LLM about how to assign tags"
                }
            },
            "required": ["data", "field", "tags"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let data = params["data"]
            .as_array()
            .ok_or_else(|| anyhow!("tag_with_llm: 'data' must be an array"))?
            .clone();

        let field = params["field"]
            .as_str()
            .ok_or_else(|| anyhow!("tag_with_llm: 'field' is required"))?;

        let tags: Vec<String> = params["tags"]
            .as_array()
            .ok_or_else(|| anyhow!("tag_with_llm: 'tags' must be an array"))?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        let extra_instructions = params["instructions"].as_str().unwrap_or("");

        let tags_list = tags.join(", ");
        let system_prompt = format!(
            "You are a classification assistant. Given a job description, assign zero or more tags \
             from the following list: [{tags_list}]. \
             A person can have multiple tags. Only use tags from the provided list. \
             {extra_instructions}\
             Respond with a JSON object with a single key 'tags' containing an array of strings."
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["tags"],
            "additionalProperties": false
        });

        info!(
            items = data.len(),
            field = %field,
            tags = ?tags,
            "tag_with_llm: start"
        );
        ctx.log(format!(
            "tag_with_llm: tagging {} items by field '{field}'",
            data.len()
        ))
        .await;

        let mut tagged: Vec<Value> = Vec::with_capacity(data.len());
        for (i, item) in data.iter().enumerate() {
            let field_value = item[field]
                .as_str()
                .unwrap_or("")
                .to_string();

            if field_value.is_empty() {
                let mut obj = item.clone();
                obj["tags"] = json!([]);
                tagged.push(obj);
                continue;
            }

            let result = ctx
                .llm
                .complete_structured(
                    &system_prompt,
                    &format!("Job description: {field_value}"),
                    "tagging_result",
                    schema.clone(),
                    Some("google/gemini-3.1-flash-lite-preview"),
                )
                .await;

            let assigned_tags = match result {
                Ok(v) => {
                    debug!(item = i, raw_tags = ?v["tags"], "tag_with_llm: LLM response");
                    v["tags"].clone()
                }
                Err(e) => {
                    warn!(item = i, error = %e, "tag_with_llm: LLM error, skipping item");
                    ctx.log(format!("tag_with_llm: error on item {i}: {e}")).await;
                    json!([])
                }
            };

            // Validate tags are from allowed list
            let valid_tags: Vec<Value> = assigned_tags
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|t| {
                            t.as_str()
                                .map(|s| tags.contains(&s.to_string()))
                                .unwrap_or(false)
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();

            let tag_strs: Vec<&str> = valid_tags.iter().filter_map(|v| v.as_str()).collect();
            debug!(item = i, tags = ?tag_strs, "tag_with_llm: item tagged");

            let mut obj = item.clone();
            obj["tags"] = Value::Array(valid_tags);
            tagged.push(obj);

            if (i + 1) % 10 == 0 || i + 1 == data.len() {
                info!(progress = i + 1, total = data.len(), "tag_with_llm: progress");
                ctx.log(format!("tag_with_llm: {}/{} items tagged", i + 1, data.len()))
                    .await;
            }
        }

        info!(tagged = tagged.len(), "tag_with_llm: done");
        Ok(Value::Array(tagged))
    }
}
