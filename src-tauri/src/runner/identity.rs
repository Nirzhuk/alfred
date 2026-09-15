use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(super) fn format_identity(data: &Value) -> String {
    let sections: Vec<String> = [
        ("role", "Role"),
        ("personality", "Personality"),
        ("instructions", "Standing instructions"),
    ]
    .into_iter()
    .filter_map(|(key, heading)| {
        data.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| format!("### {heading}\n{text}"))
    })
    .collect();
    if sections.is_empty() {
        return String::new();
    }
    let label = data
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or("Identity");
    format!("## Agent identity: {label}\n\n{}", sections.join("\n\n"))
}

pub(super) fn with_upstream_identities(
    prompt: &str,
    agent_id: &str,
    nodes: &[Value],
    edges: &[Value],
) -> String {
    let node_ids: HashSet<&str> = nodes
        .iter()
        .filter_map(|node| node.get("id").and_then(Value::as_str))
        .collect();
    let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in edges {
        if let (Some(source), Some(target)) = (
            edge.get("source").and_then(Value::as_str),
            edge.get("target").and_then(Value::as_str),
        ) {
            if node_ids.contains(source) && node_ids.contains(target) {
                incoming.entry(target).or_default().push(source);
            }
        }
    }

    let mut ancestors = HashSet::new();
    let mut pending = vec![agent_id];
    while let Some(node_id) = pending.pop() {
        if ancestors.insert(node_id) {
            pending.extend(incoming.get(node_id).into_iter().flatten().copied());
        }
    }

    let identities: Vec<String> = nodes
        .iter()
        .filter(|node| node.get("type").and_then(Value::as_str) == Some("identity"))
        .filter(|node| {
            node.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| ancestors.contains(id))
        })
        .map(|node| format_identity(&node["data"]))
        .filter(|identity| !identity.is_empty())
        .collect();
    if identities.is_empty() {
        return prompt.to_string();
    }
    let identity = identities.join("\n\n");
    if prompt.trim().is_empty() {
        identity
    } else {
        format!("{identity}\n\n{prompt}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: &str, node_type: &str) -> Value {
        json!({ "id": id, "type": node_type, "data": { "label": id, "role": id } })
    }

    fn edge(source: &str, target: &str) -> Value {
        json!({ "source": source, "target": target })
    }

    #[test]
    fn identity_formats_only_configured_fields_and_preserves_instructions() {
        let formatted = format_identity(&json!({
            "label": " Editor ",
            "role": " Senior editor ",
            "personality": " Direct and warm. ",
            "instructions": "Check sources.\nKeep literal {{output}} references."
        }));
        assert_eq!(formatted, "## Agent identity: Editor\n\n### Role\nSenior editor\n\n### Personality\nDirect and warm.\n\n### Standing instructions\nCheck sources.\nKeep literal {{output}} references.");
        assert_eq!(
            format_identity(&json!({ "role": " ", "instructions": null })),
            ""
        );
        assert_eq!(
            format_identity(&json!({ "role": 123, "personality": [] })),
            ""
        );
        assert_eq!(
            format_identity(&json!({ "instructions": "Always check facts." })),
            "## Agent identity: Identity\n\n### Standing instructions\nAlways check facts."
        );
    }

    #[test]
    fn identity_survives_intermediate_input_and_template_steps() {
        let nodes = vec![
            node("editor", "identity"),
            node("input", "input"),
            node("template", "template"),
            node("agent", "agent"),
        ];
        let edges = vec![
            edge("editor", "input"),
            edge("input", "template"),
            edge("template", "agent"),
        ];
        let prompt = with_upstream_identities("Replaced task context", "agent", &nodes, &edges);
        assert!(prompt.starts_with("## Agent identity: editor"));
        assert!(prompt.ends_with("\n\nReplaced task context"));
    }

    #[test]
    fn identity_only_reaches_connected_downstream_agents() {
        let nodes = vec![
            node("editor", "identity"),
            node("agent", "agent"),
            node("custom", "customAgent"),
            node("unrelated", "agent"),
            node("later", "identity"),
        ];
        let edges = vec![
            edge("editor", "agent"),
            edge("agent", "custom"),
            edge("custom", "later"),
        ];
        for agent_id in ["agent", "custom"] {
            let prompt = with_upstream_identities("Task", agent_id, &nodes, &edges);
            assert!(prompt.contains("Agent identity: editor"));
            assert!(!prompt.contains("Agent identity: later"));
        }
        assert_eq!(
            with_upstream_identities("Task", "unrelated", &nodes, &edges),
            "Task"
        );
    }

    #[test]
    fn merged_paths_include_each_identity_once_in_saved_order() {
        let nodes = vec![
            node("editor", "identity"),
            node("voice", "identity"),
            node("left", "template"),
            node("right", "template"),
            node("agent", "agent"),
        ];
        let edges = vec![
            edge("editor", "left"),
            edge("editor", "right"),
            edge("voice", "right"),
            edge("left", "agent"),
            edge("right", "agent"),
            edge("right", "left"),
            edge("left", "right"),
        ];
        let prompt = with_upstream_identities("Task", "agent", &nodes, &edges);
        assert_eq!(prompt.matches("Agent identity: editor").count(), 1);
        assert_eq!(prompt.matches("Agent identity: voice").count(), 1);
        assert!(prompt.find("identity: editor") < prompt.find("identity: voice"));
    }

    #[test]
    fn empty_identities_and_dangling_edges_leave_prompt_unchanged() {
        let nodes = vec![
            json!({ "id": "empty", "type": "identity", "data": {} }),
            node("isolated", "identity"),
            node("agent", "agent"),
        ];
        let edges = vec![
            edge("empty", "agent"),
            edge("isolated", "missing"),
            edge("missing", "agent"),
        ];
        assert_eq!(
            with_upstream_identities("Task", "agent", &nodes, &edges),
            "Task"
        );
        assert_eq!(with_upstream_identities("", "agent", &nodes, &edges), "");
    }
}
