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

/// Where a Jira site answers from. Atlassian's cloud is always the origin.
/// A self-hosted Jira can live under a path (`https://host/jira`): that path
/// is kept, and whatever follows it in a pasted page address
/// (`/browse/ABC-1`, `/secure/Dashboard.jspa`) is dropped.
pub fn jira_base(input: &str) -> String {
    const PAGES: [&str; 12] = [
        "browse",
        "secure",
        "projects",
        "issues",
        "rest",
        "plugins",
        "servicedesk",
        "software",
        "dashboard",
        "login",
        "wiki",
        "s",
    ];
    let origin = strip_url(input);
    let host = origin.split_once("://").map_or("", |(_, host)| host);
    if host.ends_with(".atlassian.net") || host.ends_with(".jira.com") {
        return origin;
    }
    let rest = input
        .trim()
        .split_once("://")
        .map_or(input.trim(), |(_, r)| r);
    let path = rest.split(['?', '#']).next().unwrap_or_default();
    let kept: Vec<&str> = path
        .split('/')
        .skip(1)
        .take_while(|segment| {
            !segment.is_empty()
                && !segment.contains('.')
                && !PAGES.contains(&segment.to_ascii_lowercase().as_str())
        })
        .collect();
    if kept.is_empty() {
        origin
    } else {
        format!("{origin}/{}", kept.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_self_hosted_jira_keeps_the_path_it_lives_under() {
        for (typed, base) in [
            (
                "https://acme.atlassian.net/jira/software/c/projects/X",
                "https://acme.atlassian.net",
            ),
            ("acme.atlassian.net", "https://acme.atlassian.net"),
            ("https://tools.acme.com/jira", "https://tools.acme.com/jira"),
            (
                "https://tools.acme.com/jira/",
                "https://tools.acme.com/jira",
            ),
            (
                "https://tools.acme.com/jira/browse/ABC-1",
                "https://tools.acme.com/jira",
            ),
            (
                "https://tools.acme.com/jira/secure/Dashboard.jspa",
                "https://tools.acme.com/jira",
            ),
            (
                "https://jira.acme.com/browse/ABC-1?x=1",
                "https://jira.acme.com",
            ),
            ("http://jira.acme.com:8080", "http://jira.acme.com:8080"),
        ] {
            assert_eq!(jira_base(typed), base, "{typed}");
        }
    }

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
