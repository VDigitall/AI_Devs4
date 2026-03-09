use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub ag3nts_api_key: String,
    pub openrouter_api_key: String,
    pub openrouter_model: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        let ag3nts_api_key = std::env::var("AG3NTS_API_KEY")
            .context("AG3NTS_API_KEY not set in environment or .env file")?;

        let openrouter_api_key = std::env::var("OPENROUTER_API_KEY")
            .context("OPENROUTER_API_KEY not set in environment or .env file")?;

        let openrouter_model = std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());

        Ok(Self {
            ag3nts_api_key,
            openrouter_api_key,
            openrouter_model,
        })
    }
}
