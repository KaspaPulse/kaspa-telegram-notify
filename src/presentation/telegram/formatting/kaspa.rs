pub struct KaspaFormatter;
impl KaspaFormatter {
    pub fn format_difficulty(val: f64) -> String {
        if val <= 0.0 {
            return "0.00".to_string();
        }
        if val >= 1e15 {
            format!("{:.2} P", val / 1e15)
        } else if val >= 1e12 {
            format!("{:.2} T", val / 1e12)
        } else if val >= 1e9 {
            format!("{:.2} G", val / 1e9)
        } else {
            format!("{:.2}", val)
        }
    }
    pub fn format_hashrate(h: f64) -> String {
        if h >= 1e15 {
            format!("{:.2} PH/s", h / 1e15)
        } else if h >= 1e12 {
            format!("{:.2} TH/s", h / 1e12)
        } else if h >= 1e9 {
            format!("{:.2} GH/s", h / 1e9)
        } else if h >= 1e6 {
            format!("{:.2} MH/s", h / 1e6)
        } else {
            format!("{:.2} H/s", h)
        }
    }
}
