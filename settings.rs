use serde::Deserialize;

// Enterprise Configuration Structure (Ready for Vault or Advanced Env Management)
#[derive(Debug, Deserialize)]
pub struct AppSettings {
    pub database_url: String,
    pub bot_token: String,
    pub ai_api_key: String,
}

impl AppSettings {
    pub fn init() -> Result<Self, envy::Error> {
        envy::from_env::<AppSettings>()
    }
}
