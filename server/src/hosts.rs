//! Allowed-hostname patterns per site, after Liwan (Apache-2.0, see NOTICE).
//! A pattern is an exact host or `*.example.com`, which matches subdomains
//! only. An empty list allows every host.

/// Lowercases and validates one pattern. `Ok(None)` for a blank one.
pub fn normalize(pattern: &str) -> Result<Option<String>, String> {
    let pattern = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    if pattern.is_empty() {
        return Ok(None);
    }
    let host = pattern.strip_prefix("*.").unwrap_or(&pattern);
    if host.contains('*') || !is_valid(host) {
        return Err(format!("invalid hostname: {pattern}"));
    }
    Ok(Some(pattern))
}

pub fn allowed(host: &str, patterns: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    patterns.is_empty()
        || patterns.iter().any(|p| match p.strip_prefix("*.") {
            Some(suffix) => host
                .strip_suffix(suffix)
                .is_some_and(|rest| rest.len() > 1 && rest.ends_with('.')),
            None => host == *p,
        })
}

fn is_valid(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_match_exact_and_wildcard_hosts() {
        let p = vec!["example.com".to_string(), "*.example.org".to_string()];
        assert!(allowed("example.com", &p));
        assert!(allowed("Example.COM.", &p));
        assert!(allowed("www.example.org", &p));
        assert!(allowed("a.b.example.org", &p));
        assert!(!allowed("www.example.com", &p));
        assert!(!allowed("example.org", &p));
        assert!(!allowed("badexample.org", &p));
        assert!(allowed("anything.net", &[]));
    }

    #[test]
    fn invalid_patterns_are_rejected() {
        assert_eq!(
            normalize(" Example.COM. ").unwrap(),
            Some("example.com".into())
        );
        assert_eq!(
            normalize("*.Example.com").unwrap(),
            Some("*.example.com".into())
        );
        assert_eq!(normalize("  ").unwrap(), None);
        assert!(normalize("example.*").is_err());
        assert!(normalize("*example.com").is_err());
        assert!(normalize("foo.*.example.com").is_err());
        assert!(normalize("-example.com").is_err());
        assert!(normalize("https://example.com").is_err());
    }
}
