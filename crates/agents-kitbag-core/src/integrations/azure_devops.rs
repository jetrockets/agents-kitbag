use super::*;

/// The organization from `https://dev.azure.com/Contoso/...` or the older
/// `https://contoso.visualstudio.com/...`, or the name as typed.
fn organization(input: &str) -> String {
    let input = input.trim();
    if let Some((_, rest)) = input.split_once("dev.azure.com/") {
        return rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .to_owned();
    }
    let host = input
        .split_once("://")
        .map_or(input, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    match host.strip_suffix(".visualstudio.com") {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => input.to_owned(),
    }
}

/// The organization a configured server was given: the first argument after
/// the package that is not a flag.
fn configured(server: &ServerConfig) -> Option<&str> {
    let args = match crate::runner::inner_command(server) {
        Some((_, args)) => args,
        None => &server.args,
    };
    args.iter()
        .skip(1)
        .map(String::as_str)
        .find(|a| !a.starts_with('-') && !a.starts_with('@') && !a.ends_with(".js"))
}

pub static AZURE_DEVOPS: Integration = Integration {
    key: "azure-devops",
    name: "Azure DevOps",
    instances: Instances::Multi {
        prefix: "ado-",
        noun: "organization",
    },
    fields: &[Field {
        id: "org",
        label: "Organization",
        hint: "https://dev.azure.com/YourOrg or just YourOrg",
        kind: FieldKind::Text,
    }],
    steps: &[
        "No token is needed here.",
        "On first use, a browser opens for the Microsoft account login.",
    ],
    token_url: None,
    token: TokenSource::None,
    launcher: Launcher::Npx,
    validate: |values, _, _| match organization(&required(values, "org", "Organization")?) {
        org if org.is_empty() => Err("Organization is required".to_owned()),
        org => Ok(format!("organization: {org}")),
    },
    // https://github.com/microsoft/azure-devops-mcp
    build: |values, _, packages| {
        packages.entry("@azure-devops/mcp", &[&organization(value(values, "org"))])
    },
    describe: |server| {
        vec![(
            "Organization",
            configured(server).unwrap_or("unknown").to_owned(),
        )]
    },
    prefill: |server| {
        configured(server)
            .map(|org| ("org".to_owned(), org.to_owned()))
            .into_iter()
            .collect()
    },
    check: |_, _| Health::skip("Browser sign-in, nothing to check"),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_organization_is_read_from_a_url_or_taken_as_typed() {
        assert_eq!(
            organization("https://dev.azure.com/Contoso/Project/_git/x"),
            "Contoso"
        );
        assert_eq!(organization("dev.azure.com/Contoso"), "Contoso");
        assert_eq!(organization("  Contoso "), "Contoso");
        // The older address, still in bookmarks and in what people paste.
        assert_eq!(
            organization("https://contoso.visualstudio.com/Project/_git/x"),
            "contoso"
        );
        assert_eq!(organization("contoso.visualstudio.com"), "contoso");
    }

    #[test]
    fn the_configured_organization_is_found_on_both_platforms() {
        let mac = ServerConfig::new("npx", ["-y", "@azure-devops/mcp", "Contoso"]);
        assert_eq!(configured(&mac), Some("Contoso"));
        let windows = ServerConfig::new(
            r"C:\Program Files\nodejs\node.exe",
            [
                r"C:\npm\node_modules\@azure-devops\mcp\dist\index.js",
                "Contoso",
            ],
        );
        assert_eq!(configured(&windows), Some("Contoso"));
    }
}
