use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use super::{Tool, ToolContext};

const NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/reverse";
const USER_AGENT: &str = "ai_devs4/1.0";

// Nominatim policy: max 1 request/second, no parallel requests.
// The mutex serialises all callers; the sleep enforces the 1 s gap.
const MIN_INTERVAL: Duration = Duration::from_millis(1100); // 10 % buffer over 1 s

static RATE_LIMITER: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn rate_limiter() -> &'static Mutex<Option<Instant>> {
    RATE_LIMITER.get_or_init(|| Mutex::new(None))
}

pub struct GeocodeReverseTool;

#[async_trait]
impl Tool for GeocodeReverseTool {
    fn name(&self) -> &str {
        "geocode_reverse"
    }

    fn description(&self) -> &str {
        "Reverse geocode a coordinate pair (latitude + longitude) into a human-readable address \
         using the Nominatim / OpenStreetMap API. Returns a JSON object with fields such as \
         'display_name', 'address' (road, city, country, postcode, …), and the raw 'lat'/'lon' \
         echo from the service."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "lat": {
                    "type": "number",
                    "description": "Latitude in decimal degrees (e.g. 52.2297)"
                },
                "lon": {
                    "type": "number",
                    "description": "Longitude in decimal degrees (e.g. 21.0122)"
                }
            },
            "required": ["lat", "lon"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let lat = params["lat"]
            .as_f64()
            .ok_or_else(|| anyhow!("geocode_reverse: 'lat' must be a number"))?;

        let lon = params["lon"]
            .as_f64()
            .ok_or_else(|| anyhow!("geocode_reverse: 'lon' must be a number"))?;

        if !(-90.0..=90.0).contains(&lat) {
            return Err(anyhow!(
                "geocode_reverse: 'lat' must be between -90 and 90, got {lat}"
            ));
        }
        if !(-180.0..=180.0).contains(&lon) {
            return Err(anyhow!(
                "geocode_reverse: 'lon' must be between -180 and 180, got {lon}"
            ));
        }

        info!(lat, lon, "geocode_reverse: start");
        ctx.log(format!("geocode_reverse: reverse geocoding ({lat}, {lon})"))
            .await;

        // ── Rate-limit: serialise all calls and enforce ≥ MIN_INTERVAL gap ──
        //
        // The mutex is held across the entire HTTP call so that a second caller
        // waiting on the lock cannot proceed until the first request finishes,
        // satisfying Nominatim's "no parallel requests" requirement in addition
        // to the 1 request/second limit.
        let mut last_request = rate_limiter().lock().await;

        if let Some(last) = *last_request {
            let elapsed = last.elapsed();
            if elapsed < MIN_INTERVAL {
                let wait = MIN_INTERVAL - elapsed;
                debug!(wait_ms = wait.as_millis(), "geocode_reverse: rate-limit wait");
                ctx.log(format!(
                    "geocode_reverse: rate-limit — waiting {}ms",
                    wait.as_millis()
                ))
                .await;
                sleep(wait).await;
            }
        }

        // ── HTTP request ─────────────────────────────────────────────────────
        let response = ctx
            .http
            .get(NOMINATIM_URL)
            .header("User-Agent", USER_AGENT)
            .query(&[
                ("lat", lat.to_string()),
                ("lon", lon.to_string()),
                ("format", "json".to_string()),
                ("addressdetails", "1".to_string()),
            ])
            .send()
            .await
            .map_err(|e| anyhow!("geocode_reverse: HTTP request failed: {e}"))?;

        // Record the timestamp as soon as the request is sent / response begins.
        *last_request = Some(Instant::now());
        drop(last_request); // release the lock before processing the body

        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        debug!(status = %status, body = %text, "geocode_reverse: raw response");

        if !status.is_success() {
            warn!(status = %status, body = %text, "geocode_reverse: non-success response");
            ctx.log(format!("geocode_reverse: HTTP {status} — {text}"))
                .await;
            return Err(anyhow!("geocode_reverse: HTTP {status}"));
        }

        let parsed: Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("geocode_reverse: failed to parse response JSON: {e}"))?;

        if let Some(err) = parsed["error"].as_str() {
            warn!(error = %err, "geocode_reverse: API error");
            ctx.log(format!("geocode_reverse: API error — {err}")).await;
            return Err(anyhow!("geocode_reverse: API error: {err}"));
        }

        let display_name = parsed["display_name"].as_str().unwrap_or("(unknown)");
        info!(display_name = %display_name, "geocode_reverse: done");
        ctx.log(format!("geocode_reverse: resolved → {display_name}"))
            .await;

        Ok(parsed)
    }
}
