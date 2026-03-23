use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::fs;
use tracing::info;

use super::{Tool, ToolContext};

pub struct EvaluateSensorsTool;

// ── Valid ranges for active sensors ──────────────────────────────────────────

const TEMP_MIN: f64 = 553.0;
const TEMP_MAX: f64 = 873.0;
const PRESSURE_MIN: f64 = 60.0;
const PRESSURE_MAX: f64 = 160.0;
const WATER_MIN: f64 = 5.0;
const WATER_MAX: f64 = 15.0;
const VOLTAGE_MIN: f64 = 229.0;
const VOLTAGE_MAX: f64 = 231.0;
const HUMIDITY_MIN: f64 = 40.0;
const HUMIDITY_MAX: f64 = 80.0;

/// Map a sensor type token to its corresponding JSON field name.
fn sensor_field(sensor: &str) -> Option<&'static str> {
    match sensor.trim().to_lowercase().as_str() {
        "temperature" => Some("temperature_K"),
        "pressure" => Some("pressure_bar"),
        "water" => Some("water_level_meters"),
        "voltage" => Some("voltage_supply_v"),
        "humidity" => Some("humidity_percent"),
        _ => None,
    }
}

/// All measurement fields and their expected valid ranges.
fn all_measurement_fields() -> &'static [(&'static str, f64, f64)] {
    &[
        ("temperature_K", TEMP_MIN, TEMP_MAX),
        ("pressure_bar", PRESSURE_MIN, PRESSURE_MAX),
        ("water_level_meters", WATER_MIN, WATER_MAX),
        ("voltage_supply_v", VOLTAGE_MIN, VOLTAGE_MAX),
        ("humidity_percent", HUMIDITY_MIN, HUMIDITY_MAX),
    ]
}

/// Return `true` when the reading is valid, `false` if it's an anomaly.
///
/// Anomalies:
/// 1. An active sensor's value falls outside the valid range.
/// 2. An inactive sensor reports a non-zero value.
fn is_valid(record: &Value) -> bool {
    let sensor_type = match record["sensor_type"].as_str() {
        Some(s) => s,
        None => return false,
    };

    // Build the set of active field names.
    let active_fields: Vec<&'static str> = sensor_type
        .split('/')
        .filter_map(|token| sensor_field(token))
        .collect();

    for (field, min, max) in all_measurement_fields() {
        let value = match record[*field].as_f64() {
            Some(v) => v,
            None => return false,
        };

        let is_active = active_fields.contains(field);

        if is_active {
            // Active sensor must be within the valid range.
            if value < *min || value > *max {
                return false;
            }
        } else {
            // Inactive sensor must report exactly 0.
            if value != 0.0 {
                return false;
            }
        }
    }

    true
}

#[async_trait]
impl Tool for EvaluateSensorsTool {
    fn name(&self) -> &str {
        "evaluate_sensors"
    }

    fn description(&self) -> &str {
        "Iterates over all JSON files in the given sensor log directory, validates each reading \
         against sensor-type rules and allowed ranges, and returns a list of evaluation results. \
         Each result contains the filename, whether the data is valid, and the operator note. \
         Results are also persisted to 'artifacts/sensor_static_eval.json'.\n\
         Active-sensor valid ranges: temperature_K 553-873, pressure_bar 60-160, \
         water_level_meters 5.0-15.0, voltage_supply_v 229.0-231.0, humidity_percent 40.0-80.0.\n\
         Anomalies: value out of range for an active sensor, OR a non-zero value on an inactive sensor."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "dir": {
                    "type": "string",
                    "description": "Path to the directory containing sensor JSON files. Defaults to 'artifacts/sensors/'"
                },
                "output_file": {
                    "type": "string",
                    "description": "Path where the evaluation JSON will be written. Defaults to 'artifacts/sensor_static_eval.json'"
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let dir = params["dir"]
            .as_str()
            .unwrap_or("artifacts/sensors/")
            .to_string();

        let output_file = params["output_file"]
            .as_str()
            .unwrap_or("artifacts/sensor_static_eval.json")
            .to_string();

        ctx.log(format!("evaluate_sensors: scanning '{dir}'")).await;

        // Collect and sort JSON filenames.
        let mut entries: Vec<String> = fs::read_dir(&dir)
            .with_context(|| format!("evaluate_sensors: cannot open directory '{dir}'"))?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".json") {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();

        entries.sort();

        let total = entries.len();
        info!(total, dir = %dir, "evaluate_sensors: files found");

        let mut results: Vec<Value> = Vec::with_capacity(total);
        let mut anomaly_count = 0usize;

        for filename in &entries {
            let path = format!("{}/{}", dir.trim_end_matches('/'), filename);
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("evaluate_sensors: cannot read '{path}'"))?;

            let record: Value = serde_json::from_str(&raw)
                .with_context(|| format!("evaluate_sensors: invalid JSON in '{path}'"))?;

            let data_is_valid = is_valid(&record);
            if !data_is_valid {
                anomaly_count += 1;
            }

            let operator_notes = record["operator_notes"]
                .as_str()
                .unwrap_or("")
                .to_string();

            results.push(json!({
                "filename": filename,
                "data_is_valid": data_is_valid,
                "operator_notes": operator_notes
            }));
        }

        // Persist results.
        let output_json = serde_json::to_string_pretty(&results)?;
        fs::write(&output_file, &output_json)
            .with_context(|| format!("evaluate_sensors: cannot write '{output_file}'"))?;

        info!(total, anomaly_count, output = %output_file, "evaluate_sensors: done");
        ctx.log(format!(
            "evaluate_sensors: evaluated {total} files, {anomaly_count} anomalies — written to '{output_file}'"
        ))
        .await;

        Ok(Value::Array(results))
    }
}
