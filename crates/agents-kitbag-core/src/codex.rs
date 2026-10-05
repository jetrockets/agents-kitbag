//! Codex's `config.toml`: the servers under `mcp_servers`, read and changed
//! without touching anything else in the file, comments included.

use serde_json::{Map, Value as Json};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, TableLike, Value};

use crate::config::ServerConfig;

const SERVERS: &str = "mcp_servers";

fn parse(text: &str) -> Result<DocumentMut, String> {
    text.parse()
        .map_err(|error| format!("The config file is not valid TOML: {error}"))
}

/// Every configured server, in the file's order.
pub fn servers(text: &str) -> Result<Vec<(String, ServerConfig)>, String> {
    let doc = parse(text)?;
    let Some(servers) = doc.get(SERVERS).and_then(Item::as_table_like) else {
        return Ok(Vec::new());
    };
    Ok(servers
        .iter()
        .filter_map(|(key, item)| Some((key.to_owned(), server_of(item.as_table_like()?))))
        .collect())
}

/// The file with this server added or replaced.
pub fn set_server(text: &str, key: &str, server: &ServerConfig) -> Result<String, String> {
    let mut doc = parse(text)?;
    servers_mut(&mut doc)?.insert(key, Item::Table(table_of(server)));
    Ok(doc.to_string())
}

/// The file without these servers, and the keys that were there.
pub fn remove_servers(text: &str, keys: &[String]) -> Result<(String, Vec<String>), String> {
    let mut doc = parse(text)?;
    if doc.get(SERVERS).is_none() {
        return Ok((text.to_owned(), Vec::new()));
    }
    let servers = servers_mut(&mut doc)?;
    let removed = keys
        .iter()
        .filter(|key| servers.remove(key).is_some())
        .cloned()
        .collect();
    Ok((doc.to_string(), removed))
}

fn servers_mut(doc: &mut DocumentMut) -> Result<&mut dyn TableLike, String> {
    doc.entry(SERVERS)
        .or_insert_with(|| {
            // No `[mcp_servers]` line of its own: only `[mcp_servers.<name>]`.
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_like_mut()
        .ok_or_else(|| "mcp_servers in the config file is not a table".to_owned())
}

fn server_of(table: &dyn TableLike) -> ServerConfig {
    let mut server = ServerConfig::default();
    for (key, item) in table.iter() {
        match key {
            "command" => server.command = item.as_str().unwrap_or_default().to_owned(),
            "args" => {
                server.args = item
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|arg| arg.as_str().map(str::to_owned))
                    .collect();
            }
            "env" => {
                if let Some(Json::Object(env)) = json_of(item) {
                    server.env = Some(env);
                }
            }
            _ => {
                if let Some(value) = json_of(item) {
                    server.rest.insert(key.to_owned(), value);
                }
            }
        }
    }
    server
}

fn table_of(server: &ServerConfig) -> Table {
    let mut table = Table::new();
    // A server reached by `url` has no command to write.
    if !server.command.is_empty() {
        table.insert("command", toml_edit::value(server.command.as_str()));
        table.insert(
            "args",
            toml_edit::value(server.args.iter().map(String::as_str).collect::<Array>()),
        );
    }
    for (key, value) in &server.rest {
        if let Some(value) = toml_of(value) {
            table.insert(key, Item::Value(value));
        }
    }
    if let Some(env) = &server.env {
        let mut vars = Table::new();
        for (name, value) in env {
            if let Some(value) = toml_of(value) {
                vars.insert(name, Item::Value(value));
            }
        }
        table.insert("env", Item::Table(vars));
    }
    table
}

fn json_of(item: &Item) -> Option<Json> {
    match item {
        Item::None => None,
        Item::Value(value) => Some(json_of_value(value)),
        Item::Table(table) => Some(json_of_table(table)),
        Item::ArrayOfTables(tables) => Some(Json::Array(
            tables.iter().map(|t| json_of_table(t)).collect(),
        )),
    }
}

fn json_of_table(table: &dyn TableLike) -> Json {
    Json::Object(
        table
            .iter()
            .filter_map(|(key, item)| Some((key.to_owned(), json_of(item)?)))
            .collect::<Map<String, Json>>(),
    )
}

fn json_of_value(value: &Value) -> Json {
    match value {
        Value::String(s) => Json::String(s.value().clone()),
        Value::Integer(i) => Json::from(*i.value()),
        Value::Float(f) => {
            serde_json::Number::from_f64(*f.value()).map_or(Json::Null, Json::Number)
        }
        Value::Boolean(b) => Json::Bool(*b.value()),
        Value::Datetime(d) => Json::String(d.value().to_string()),
        Value::Array(array) => Json::Array(array.iter().map(json_of_value).collect()),
        Value::InlineTable(table) => json_of_table(table),
    }
}

/// TOML has no null: a key that holds one is left out.
fn toml_of(value: &Json) -> Option<Value> {
    Some(match value {
        Json::Null => return None,
        Json::Bool(b) => Value::from(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::from(i),
            None => Value::from(n.as_f64()?),
        },
        Json::String(s) => Value::from(s.as_str()),
        Json::Array(items) => Value::Array(items.iter().filter_map(toml_of).collect()),
        Json::Object(map) => {
            let mut table = InlineTable::new();
            for (key, value) in map {
                if let Some(value) = toml_of(value) {
                    table.insert(key, value);
                }
            }
            Value::InlineTable(table)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"# my settings
model = "gpt-5"   # the default

[mcp_servers.notes]
command = "uvx"
args = ["mcp-notes"]
startup_timeout_sec = 120

[mcp_servers.notes.env]
NOTES_PORT = "27124"

[mcp_servers.design]
url = "https://mcp.example.com/mcp"

[projects."/work/app"]
trust_level = "trusted"
"#;

    fn asana() -> ServerConfig {
        ServerConfig::new("npx", ["-y", "@roychri/mcp-server-asana@beta"])
            .with_env(&[("ASANA_ACCESS_TOKEN", "t")])
    }

    #[test]
    fn an_empty_file_has_no_servers() {
        assert_eq!(servers("").unwrap(), []);
        assert_eq!(servers("model = \"gpt-5\"\n").unwrap(), []);
    }

    #[test]
    fn servers_are_read_with_everything_they_hold() {
        let servers = servers(FILE).unwrap();
        assert_eq!(servers.len(), 2);
        let (key, notes) = &servers[0];
        assert_eq!(key, "notes");
        assert_eq!(notes.command, "uvx");
        assert_eq!(notes.args, ["mcp-notes"]);
        assert_eq!(notes.env_var("NOTES_PORT"), Some("27124"));
        assert_eq!(notes.rest["startup_timeout_sec"], 120);
        // A server reached by URL has no command.
        let (key, design) = &servers[1];
        assert_eq!(key, "design");
        assert_eq!(design.command, "");
        assert_eq!(design.rest["url"], "https://mcp.example.com/mcp");
    }

    #[test]
    fn a_first_server_is_written_as_codex_writes_one() {
        let text = set_server("", "asana", &asana()).unwrap();
        assert_eq!(
            text,
            r#"[mcp_servers.asana]
command = "npx"
args = ["-y", "@roychri/mcp-server-asana@beta"]

[mcp_servers.asana.env]
ASANA_ACCESS_TOKEN = "t"
"#
        );
    }

    #[test]
    fn everything_else_in_the_file_survives_a_write() {
        let text = set_server(FILE, "asana", &asana()).unwrap();
        for kept in [
            "# my settings",
            "model = \"gpt-5\"   # the default",
            "[mcp_servers.notes]",
            "startup_timeout_sec = 120",
            "NOTES_PORT = \"27124\"",
            "url = \"https://mcp.example.com/mcp\"",
            "[projects.\"/work/app\"]",
            "trust_level = \"trusted\"",
        ] {
            assert!(text.contains(kept), "{kept} is gone from:\n{text}");
        }
        let keys: Vec<_> = servers(&text)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["notes", "design", "asana"]);
        assert_eq!(servers(&text).unwrap()[2].1, asana());
    }

    #[test]
    fn replacing_a_server_leaves_one_of_it() {
        let plain = ServerConfig::new("npx", ["-y", "mcp-notes"]);
        let text = set_server(FILE, "notes", &plain).unwrap();
        assert_eq!(text.matches("[mcp_servers.notes]").count(), 1);
        assert!(!text.contains("NOTES_PORT"), "{text}");
        assert_eq!(servers(&text).unwrap()[0].1, plain);
    }

    #[test]
    fn a_server_read_and_written_back_keeps_what_the_app_does_not_know() {
        let (_, notes) = servers(FILE).unwrap().remove(0);
        let text = set_server(FILE, "notes", &notes).unwrap();
        assert_eq!(servers(&text).unwrap()[0].1, notes);
        let (_, design) = servers(FILE).unwrap().remove(1);
        let text = set_server(FILE, "design", &design).unwrap();
        assert_eq!(text.matches("command").count(), 1, "{text}");
        assert_eq!(servers(&text).unwrap()[1].1, design);
    }

    #[test]
    fn removing_reports_only_what_was_there() {
        let (text, removed) =
            remove_servers(FILE, &["notes".to_owned(), "ghost".to_owned()]).unwrap();
        assert_eq!(removed, ["notes"]);
        assert!(!text.contains("mcp_servers.notes"), "{text}");
        assert!(text.contains("[mcp_servers.design]"));
        assert!(text.contains("[projects.\"/work/app\"]"));

        let (text, removed) = remove_servers("model = \"x\"\n", &["notes".to_owned()]).unwrap();
        assert_eq!(removed, [] as [String; 0]);
        assert_eq!(text, "model = \"x\"\n");
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_error() {
        assert!(
            servers("[mcp_servers")
                .unwrap_err()
                .contains("not valid TOML")
        );
        assert!(set_server("[mcp_servers", "asana", &asana()).is_err());
        assert!(
            set_server("mcp_servers = 3\n", "asana", &asana())
                .unwrap_err()
                .contains("not a table")
        );
    }
}
