//! What a person may type.

/// An instance name becomes part of a config key (`jira-<name>`) and of a
/// credential's name, so it stays lowercase letters, digits and hyphens.
pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("Name cannot be empty");
    }
    let edge = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    let inner = |c: char| edge(c) || c == '-';
    let first = name.chars().next().is_some_and(edge);
    let last = name.chars().last().is_some_and(edge);
    if first && last && name.chars().all(inner) {
        Ok(())
    } else {
        Err("Lowercase letters, digits and hyphens only (e.g. acme, client-b)")
    }
}

/// A site's origin from whatever was pasted: `https://acme.atlassian.net`
/// from `https://acme.atlassian.net/jira/software/...`. A bare host gets
/// `https://`.
pub fn strip_url(input: &str) -> String {
    let input = input.trim();
    let (scheme, rest) = match input.split_once("://") {
        Some((scheme, rest)) if !scheme.is_empty() => (scheme, rest),
        _ => ("https", input),
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    format!("{scheme}://{host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lowercase_alphanumerics_and_inner_hyphens() {
        for good in ["acme", "client-b", "a", "a1", "1a-2b"] {
            assert_eq!(validate_name(good), Ok(()), "{good}");
        }
        for bad in ["", "-acme", "acme-", "Acme", "ac me", "a_b", "ünï"] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_pasted_address_is_cut_to_its_origin() {
        assert_eq!(
            strip_url("https://acme.atlassian.net/jira/software/projects"),
            "https://acme.atlassian.net"
        );
        assert_eq!(
            strip_url("http://localhost:8080/x?y#z"),
            "http://localhost:8080"
        );
        assert_eq!(
            strip_url(" acme.atlassian.net/foo "),
            "https://acme.atlassian.net"
        );
        assert_eq!(
            strip_url("https://acme.atlassian.net"),
            "https://acme.atlassian.net"
        );
    }
}
