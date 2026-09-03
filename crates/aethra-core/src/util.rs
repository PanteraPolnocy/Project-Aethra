use chrono::{DateTime, Local, SecondsFormat, Utc};

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn to_rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

/// Local calendar day, used as the budget ledger key.
pub fn today_local() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Cuts a string at a character boundary, appending a marker when truncated.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push_str("\n[... truncated ...]");
    out
}

pub fn clamp01(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_is_char_safe() {
        let s = "zazolc gesla jazn - 12345";
        assert_eq!(truncate_chars(s, 100), s);
        let t = truncate_chars(s, 5);
        assert!(t.starts_with("zazol"));
        assert!(t.ends_with("[... truncated ...]"));
    }

    #[test]
    fn clamp_handles_nan() {
        assert_eq!(clamp01(f64::NAN), 0.0);
        assert_eq!(clamp01(1.7), 1.0);
        assert_eq!(clamp01(-0.2), 0.0);
    }
}
