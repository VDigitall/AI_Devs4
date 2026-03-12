use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{json, Value};
use std::path::Path;
use tracing::{debug, info};

use crate::llm::Message;
use super::{Tool, ToolContext};

const VISION_MODEL: &str = "google/gemini-2.0-flash-001";

pub struct DescribeImageTool;

#[async_trait]
impl Tool for DescribeImageTool {
    fn name(&self) -> &str {
        "describe_image"
    }

    fn description(&self) -> &str {
        "Describe the content of an image using a vision LLM and save the description as a \
         Markdown file in the 'artifacts/' directory. \
         Input can be a local file path (must start with 'artifacts/') or an HTTP/HTTPS URL. \
         Returns the Markdown description and the path where it was saved."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filepath": {
                    "type": "string",
                    "description": "Path to a local image file. Must start with 'artifacts/' \
                                    (e.g. 'artifacts/photo.jpg'). Mutually exclusive with 'url'."
                },
                "url": {
                    "type": "string",
                    "description": "HTTP/HTTPS URL of the image to describe. \
                                    Mutually exclusive with 'filepath'."
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional custom prompt / question about the image. \
                                    Defaults to a comprehensive description request."
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let filepath = params["filepath"].as_str();
        let url = params["url"].as_str();
        let prompt = params["prompt"]
            .as_str()
            .unwrap_or("Describe this image in detail. Include all visible objects, text, colors, \
                        spatial relationships, and any other notable elements. \
                        Format the output as structured Markdown.");

        let (image_url, stem) = match (filepath, url) {
            (Some(_), Some(_)) => {
                return Err(anyhow!("describe_image: provide either 'filepath' or 'url', not both"));
            }

            (Some(path), None) => {
                validate_artifacts_path(path)?;
                ctx.log(format!("describe_image: reading image from file '{path}'")).await;
                debug!(path = %path, "describe_image: reading local file");

                let bytes = std::fs::read(path)
                    .map_err(|e| anyhow!("describe_image: cannot read '{path}' – {e}"))?;

                let mime = mime_from_path(path)?;
                let b64 = BASE64.encode(&bytes);
                let data_url = format!("data:{mime};base64,{b64}");

                let stem = Path::new(path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("image")
                    .to_string();

                info!(path = %path, bytes = bytes.len(), "describe_image: file loaded");
                ctx.log(format!("describe_image: loaded {} bytes from '{path}'", bytes.len())).await;

                (data_url, stem)
            }

            (None, Some(remote_url)) => {
                ctx.log(format!("describe_image: using image URL {remote_url}")).await;
                debug!(url = %remote_url, "describe_image: using remote URL");

                let stem = url_stem(remote_url);
                (remote_url.to_string(), stem)
            }

            (None, None) => {
                return Err(anyhow!("describe_image: one of 'filepath' or 'url' is required"));
            }
        };

        ctx.log(format!("describe_image: calling vision model ({VISION_MODEL})...")).await;

        let message = Message::user_with_images(prompt, vec![image_url]);
        let resp = ctx
            .llm
            .chat(vec![message], None, None, Some(VISION_MODEL))
            .await?;

        let description = resp
            .content
            .ok_or_else(|| anyhow!("describe_image: no content in API response"))?;

        info!(stem = %stem, chars = description.len(), "describe_image: got description");
        ctx.log(format!("describe_image: received description ({} chars)", description.len())).await;

        std::fs::create_dir_all("artifacts")
            .map_err(|e| anyhow!("describe_image: cannot create 'artifacts/' directory – {e}"))?;

        let output_path = format!("artifacts/{stem}.md");
        std::fs::write(&output_path, &description)
            .map_err(|e| anyhow!("describe_image: cannot write '{output_path}' – {e}"))?;

        info!(path = %output_path, "describe_image: saved description");
        ctx.log(format!("describe_image: saved description to '{output_path}'")).await;

        Ok(json!({
            "description": description,
            "saved_to": output_path
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
            "describe_image: file access is restricted to the 'artifacts/' directory, got '{path}'"
        ));
    }

    if canonical
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "describe_image: path traversal is not allowed (got '{path}')"
        ));
    }

    Ok(())
}

fn mime_from_path(path: &str) -> Result<&'static str> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "png"          => Ok("image/png"),
        "webp"         => Ok("image/webp"),
        "gif"          => Ok("image/gif"),
        other => Err(anyhow!(
            "describe_image: unsupported image extension '.{other}'. \
             Supported: jpg, jpeg, png, webp, gif"
        )),
    }
}

fn url_stem(url: &str) -> String {
    url.rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .and_then(|s| Path::new(s).file_stem()?.to_str())
        .unwrap_or("image")
        .to_string()
}
