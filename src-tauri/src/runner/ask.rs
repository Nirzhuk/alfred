//! `{{ask:Name}}` / `{{ask:Name|default}}` placeholders in Input and Template
//! nodes. A manual run fills them from the Run dialog; unattended runs
//! (schedule, tray, app events) fall back to the default, else empty text.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

static ASK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*ask:([^}|]+?)\s*(?:\|([^}]*))?\}\}").unwrap());

pub fn resolve_text(text: &str, answers: &HashMap<String, String>) -> String {
    ASK.replace_all(text, |caps: &regex::Captures| {
        let name = caps[1].trim();
        answers
            .get(name)
            .filter(|answer| !answer.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| caps.get(2).map_or("", |d| d.as_str()).trim().to_string())
    })
    .into_owned()
}

/// Resolve placeholders in every Input `prompt` and Template `template`.
pub fn resolve_nodes(nodes: &mut [Value], answers: &HashMap<String, String>) {
    for node in nodes {
        let field = match node.get("type").and_then(Value::as_str) {
            Some("prompt" | "input") => "prompt",
            Some("template") => "template",
            _ => continue,
        };
        if let Some(Value::String(text)) = node.pointer_mut(&format!("/data/{field}")) {
            *text = resolve_text(text, answers);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn answers_then_defaults_then_empty() {
        let answers = HashMap::from([("Topic".to_string(), "Rust".to_string())]);
        assert_eq!(
            resolve_text("{{ask:Topic}} / {{ ask: Tone | calm }} / {{ask:Missing}}", &answers),
            "Rust / calm / "
        );
        let blank = HashMap::from([("Tone".to_string(), "  ".to_string())]);
        assert_eq!(resolve_text("{{ask:Tone|calm}}", &blank), "calm");
        assert_eq!(resolve_text("{{output}} {{context}}", &answers), "{{output}} {{context}}");
    }

    #[test]
    fn resolves_input_and_template_nodes_only() {
        let answers = HashMap::from([("Topic".to_string(), "Rust".to_string())]);
        let mut nodes = vec![
            json!({"type": "input", "data": {"prompt": "About {{ask:Topic}}"}}),
            json!({"type": "template", "data": {"template": "{{ask:Topic}} {{output}}"}}),
            json!({"type": "script", "data": {"command": "{{ask:Topic}}"}}),
        ];
        resolve_nodes(&mut nodes, &answers);
        assert_eq!(nodes[0]["data"]["prompt"], "About Rust");
        assert_eq!(nodes[1]["data"]["template"], "Rust {{output}}");
        assert_eq!(nodes[2]["data"]["command"], "{{ask:Topic}}");
    }
}
