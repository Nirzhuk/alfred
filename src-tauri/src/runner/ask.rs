//! `{{ask:Name}}` / `{{ask:Name|default}}` placeholders in Input and Template
//! nodes. A manual run fills them from the Run dialog, and the MCP
//! `run_workflow` tool from its `answers` argument; unattended runs (schedule,
//! tray, app events) fall back to the default, else empty text.

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

/// The node field that may hold placeholders: Input `prompt`, Template `template`.
fn ask_field(node: &Value) -> Option<&'static str> {
    match node.get("type").and_then(Value::as_str) {
        Some("prompt" | "input") => Some("prompt"),
        Some("template") => Some("template"),
        _ => None,
    }
}

/// Resolve placeholders in every Input `prompt` and Template `template`.
pub fn resolve_nodes(nodes: &mut [Value], answers: &HashMap<String, String>) {
    for node in nodes {
        let Some(field) = ask_field(node) else { continue };
        if let Some(Value::String(text)) = node.pointer_mut(&format!("/data/{field}")) {
            *text = resolve_text(text, answers);
        }
    }
}

/// Names that would resolve to empty text: no default and no non-blank answer.
/// `{{ask:Name|}}` has an explicit empty default, so it is optional.
pub fn missing_answers(nodes: &[Value], answers: &HashMap<String, String>) -> Vec<String> {
    let mut missing = Vec::new();
    for node in nodes {
        let text = ask_field(node)
            .and_then(|field| node.pointer(&format!("/data/{field}")))
            .and_then(Value::as_str)
            .unwrap_or("");
        for caps in ASK.captures_iter(text) {
            let name = caps[1].trim();
            let answered = answers.get(name).is_some_and(|a| !a.trim().is_empty());
            if !answered && caps.get(2).is_none() && !missing.iter().any(|m| m == name) {
                missing.push(name.to_string());
            }
        }
    }
    missing
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

    #[test]
    fn missing_answers_lists_only_required_unanswered_names() {
        let nodes = vec![
            json!({"type": "input", "data": {"prompt": "{{ask:Topic}} {{ask:Repo}} {{ask:Tone|calm}} {{ask:Note|}}"}}),
            json!({"type": "template", "data": {"template": "{{ask:Repo}} {{ ask: Channel }}"}}),
            json!({"type": "script", "data": {"body": "{{ask:Ignored}}"}}),
            json!({"type": "input", "data": {}}),
        ];
        let answers = HashMap::from([
            ("Topic".to_string(), "Rust".to_string()),
            ("Channel".to_string(), "  ".to_string()),
        ]);
        assert_eq!(missing_answers(&nodes, &answers), ["Repo", "Channel"]);

        let answers = HashMap::from([
            ("Topic".to_string(), "Rust".to_string()),
            ("Repo".to_string(), "alfred".to_string()),
            ("Channel".to_string(), "dev".to_string()),
        ]);
        assert!(missing_answers(&nodes, &answers).is_empty());
    }
}
