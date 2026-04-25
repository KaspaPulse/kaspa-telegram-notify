#[derive(Debug, Clone)]
pub struct AppSettings {
    pub maintenance_mode: bool,
    pub ai_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl AppSettings {
    pub fn new() -> Self {
        Self {
            maintenance_mode: false,
            ai_enabled: true,
        }
    }
}
