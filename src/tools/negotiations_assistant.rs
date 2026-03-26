use anyhow::{anyhow, Result};
use csv::ReaderBuilder;
use std::collections::HashMap;
use tracing::{info, warn};

use super::ToolContext;

// ── Text normalisation ────────────────────────────────────────────────────────

fn normalize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ą' | 'Ą' => 'a',
            'ć' | 'Ć' => 'c',
            'ę' | 'Ę' => 'e',
            'ł' | 'Ł' => 'l',
            'ń' | 'Ń' => 'n',
            'ó' | 'Ó' => 'o',
            'ś' | 'Ś' => 's',
            'ź' | 'Ź' => 'z',
            'ż' | 'Ż' => 'z',
            c => c,
        })
        .collect::<String>()
        .to_lowercase()
}

fn tokenize(text: &str) -> Vec<String> {
    normalize(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(String::from)
        .collect()
}

// ── NegotiationsAssistant ─────────────────────────────────────────────────────

pub struct NegotiationsAssistant {
    /// (item_code, item_name)
    items: Vec<(String, String)>,
    /// city_code -> city_name
    cities: HashMap<String, String>,
    /// item_code -> Vec<city_code>
    item_cities: HashMap<String, Vec<String>>,
    /// normalized keyword -> indices into self.items
    keyword_index: HashMap<String, Vec<usize>>,
}

impl NegotiationsAssistant {
    pub fn load() -> Result<Self> {
        // items.csv — header: name,code
        let mut items: Vec<(String, String)> = Vec::new();
        {
            let mut rdr = ReaderBuilder::new().from_path("artifacts/items.csv")?;
            for result in rdr.records() {
                let rec = result?;
                if rec.len() >= 2 {
                    items.push((rec[1].trim().to_string(), rec[0].trim().to_string()));
                }
            }
        }
        info!("negotiations: loaded {} items", items.len());

        // cities.csv — header: name,code
        let mut cities: HashMap<String, String> = HashMap::new();
        {
            let mut rdr = ReaderBuilder::new().from_path("artifacts/cities.csv")?;
            for result in rdr.records() {
                let rec = result?;
                if rec.len() >= 2 {
                    cities.insert(rec[1].trim().to_string(), rec[0].trim().to_string());
                }
            }
        }
        info!("negotiations: loaded {} cities", cities.len());

        // connections.csv — header: itemCode,cityCode
        let mut item_cities: HashMap<String, Vec<String>> = HashMap::new();
        {
            let mut rdr = ReaderBuilder::new().from_path("artifacts/connections.csv")?;
            for result in rdr.records() {
                let rec = result?;
                if rec.len() >= 2 {
                    item_cities
                        .entry(rec[0].trim().to_string())
                        .or_default()
                        .push(rec[1].trim().to_string());
                }
            }
        }
        let total_links: usize = item_cities.values().map(|v| v.len()).sum();
        info!("negotiations: loaded {} item-city links", total_links);

        // Build inverted keyword index over item names
        let mut keyword_index: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, (_, name)) in items.iter().enumerate() {
            for token in tokenize(name) {
                keyword_index.entry(token).or_default().push(idx);
            }
        }

        Ok(Self { items, cities, item_cities, keyword_index })
    }

    /// Find cities that sell the item described by `query`.
    /// Returns the plain-text content to place inside `{"output": "..."}`.
    pub async fn search(&self, query: &str, ctx: &ToolContext) -> Result<String> {
        info!("negotiations: searching for '{}'", query);

        let tokens = tokenize(query);
        if tokens.is_empty() {
            return Ok("No search terms found in query.".to_string());
        }

        // Score items by number of matching tokens
        let mut scores: HashMap<usize, usize> = HashMap::new();
        for token in &tokens {
            if let Some(indices) = self.keyword_index.get(token) {
                for &idx in indices {
                    *scores.entry(idx).or_default() += 1;
                }
            }
        }

        if scores.is_empty() {
            warn!("negotiations: no keyword matches for '{}'", query);
            return Ok("No items found matching this query.".to_string());
        }

        let max_score = *scores.values().max().unwrap();
        let mut top_candidates: Vec<usize> = scores
            .iter()
            .filter(|(_, &s)| s == max_score)
            .map(|(&idx, _)| idx)
            .collect();
        top_candidates.sort_unstable();

        let best_item_idx = if top_candidates.len() == 1 {
            top_candidates[0]
        } else {
            self.llm_pick_best(query, &top_candidates, ctx).await?
        };

        let (item_code, item_name) = &self.items[best_item_idx];
        info!("negotiations: matched '{}' ({})", item_name, item_code);
        ctx.log(format!(
            "negotiations: query='{}' → item='{}' ({})",
            query, item_name, item_code
        ))
        .await;

        let city_codes = self.item_cities.get(item_code).cloned().unwrap_or_default();
        let mut city_names: Vec<String> = city_codes
            .iter()
            .filter_map(|code| self.cities.get(code).cloned())
            .collect();
        city_names.sort();
        city_names.dedup();

        let response = Self::format_response(item_name, &city_names);
        ctx.log(format!("negotiations: {} cities found", city_names.len())).await;
        Ok(response)
    }

    fn format_response(item_name: &str, cities: &[String]) -> String {
        // Leave room for the JSON wrapper {"output": "..."} — ~14 bytes
        const MAX_CONTENT: usize = 485;

        let prefix = format!("Item: {}. Cities: ", item_name);
        let mut result = prefix;
        let mut first = true;

        for city in cities {
            let addition = if first {
                city.clone()
            } else {
                format!(", {}", city)
            };
            if result.len() + addition.len() > MAX_CONTENT {
                break;
            }
            result.push_str(&addition);
            first = false;
        }

        result
    }

    async fn llm_pick_best(
        &self,
        query: &str,
        candidates: &[usize],
        ctx: &ToolContext,
    ) -> Result<usize> {
        let top: Vec<&(String, String)> = candidates.iter().take(20).map(|&i| &self.items[i]).collect();

        let numbered: String = top
            .iter()
            .enumerate()
            .map(|(i, (code, name))| format!("{}. {} ({})", i, name, code))
            .collect::<Vec<_>>()
            .join("\n");

        let system = "You are a product catalog search assistant. Given a natural language query \
            (possibly in Polish) and a numbered list of catalog items, return the 0-based index \
            of the single best matching item. Respond with ONLY the integer, nothing else.";
        let user = format!("Query: {}\n\nCandidates:\n{}", query, numbered);

        let response = ctx.llm.complete(system, &user, None).await?;
        let idx: usize = response.trim().parse().map_err(|_| {
            anyhow!("LLM returned non-integer for item pick: '{}'", response.trim())
        })?;

        if idx >= top.len() {
            warn!(
                "negotiations: LLM returned out-of-range index {}, falling back to 0",
                idx
            );
            return Ok(candidates[0]);
        }
        Ok(candidates[idx])
    }
}
