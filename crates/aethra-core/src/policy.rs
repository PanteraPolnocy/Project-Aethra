//! Network policy: which URLs the agent may fetch, and under what mode.
//!
//! Deny by default. Learning mode may only reach the configured domain list.
//! Chat mode additionally may fetch URLs the user themselves typed, because
//! that is explicit intent. IP literals, local names and non-default ports
//! are refused outright to keep the agent away from local services.

use std::collections::HashSet;

use url::{Host, Url};

use crate::config::NetworkConfig;
use crate::error::{CoreError, Result};
use crate::mode::Mode;

pub fn domain_allowed(allowed: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    allowed.iter().any(|d| {
        let d = d.trim().trim_start_matches('.').to_ascii_lowercase();
        !d.is_empty() && (host == d || host.ends_with(&format!(".{d}")))
    })
}

/// Canonical form used to compare a candidate URL with what the user typed.
pub fn normalise_url(u: &Url) -> String {
    let mut c = u.clone();
    c.set_fragment(None);
    let s = c.to_string();
    s.trim_end_matches('/').to_ascii_lowercase()
}

pub fn extract_urls(text: &str) -> Vec<Url> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"' || c == '\'' || c == '(' || c == ')') {
        let candidate = raw.trim_end_matches(['.', ',', ';', ':', '!', '?', ']', '}']);
        if candidate.starts_with("http://") || candidate.starts_with("https://") {
            if let Ok(u) = Url::parse(candidate) {
                out.push(u);
            }
        }
    }
    out
}

pub fn check_url(cfg: &NetworkConfig, mode: Mode, url: &Url, user_urls: &HashSet<String>) -> Result<()> {
    if !cfg.enabled {
        return Err(CoreError::PolicyDenied("network access is disabled".into()));
    }
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(CoreError::PolicyDenied(format!("scheme '{other}' is not allowed"))),
    }
    if let Some(port) = url.port() {
        let default_port = if url.scheme() == "https" { 443 } else { 80 };
        if port != default_port {
            return Err(CoreError::PolicyDenied("non-default ports are not allowed".into()));
        }
    }
    let host = match url.host() {
        Some(Host::Domain(d)) => d.to_ascii_lowercase(),
        Some(Host::Ipv4(_)) | Some(Host::Ipv6(_)) => {
            return Err(CoreError::PolicyDenied("IP-literal hosts are not allowed".into()))
        }
        None => return Err(CoreError::PolicyDenied("URL has no host".into())),
    };
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".lan")
        || !host.contains('.')
    {
        return Err(CoreError::PolicyDenied(format!("host '{host}' looks local")));
    }
    if domain_allowed(&cfg.allowed_domains, &host) {
        return Ok(());
    }
    if mode == Mode::Chat && cfg.allow_user_provided_urls && user_urls.contains(&normalise_url(url)) {
        return Ok(());
    }
    Err(CoreError::PolicyDenied(format!(
        "'{host}' is not on the allowed domain list{}",
        if mode == Mode::Chat { " and was not provided by the user" } else { " (learning mode)" }
    )))
}

pub fn user_url_set(text: &str) -> HashSet<String> {
    extract_urls(text).iter().map(normalise_url).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> NetworkConfig {
        NetworkConfig {
            allowed_domains: vec!["wikipedia.org".into(), "docs.rs".into()],
            ..NetworkConfig::default()
        }
    }

    #[test]
    fn allowlist_matches_subdomains_only() {
        assert!(domain_allowed(&cfg().allowed_domains, "en.wikipedia.org"));
        assert!(domain_allowed(&cfg().allowed_domains, "docs.rs"));
        assert!(!domain_allowed(&cfg().allowed_domains, "notdocs.rs"));
        assert!(!domain_allowed(&cfg().allowed_domains, "wikipedia.org.evil.com"));
    }

    #[test]
    fn learning_mode_is_allowlist_only() {
        let user = user_url_set("look at https://example.com/page please");
        let u = Url::parse("https://example.com/page").unwrap();
        assert!(check_url(&cfg(), Mode::Learning, &u, &user).is_err());
        assert!(check_url(&cfg(), Mode::Chat, &u, &user).is_ok());
        let w = Url::parse("https://en.wikipedia.org/wiki/Rust").unwrap();
        assert!(check_url(&cfg(), Mode::Learning, &w, &HashSet::new()).is_ok());
    }

    #[test]
    fn local_targets_are_refused() {
        let user = user_url_set("http://127.0.0.1:8080/ http://localhost/x http://intranet/ https://docs.rs:8443/");
        for raw in ["http://127.0.0.1:8080/", "http://localhost/x", "http://intranet/", "https://docs.rs:8443/"] {
            let u = Url::parse(raw).unwrap();
            assert!(check_url(&cfg(), Mode::Chat, &u, &user).is_err(), "{raw} should be denied");
        }
    }

    #[test]
    fn urls_are_extracted_from_prose() {
        let urls = extract_urls("See (https://docs.rs/tokio), and https://en.wikipedia.org/wiki/Foo.");
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].host_str(), Some("docs.rs"));
        assert_eq!(urls[1].path(), "/wiki/Foo");
    }
}
