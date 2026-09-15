use serde_json::Value;

pub(crate) const TASK_QUERY_MAX_BYTES: usize = 8_000;
pub(crate) const EVENT_QUERY_MAX_BYTES: usize = 2_000;
const AUTHORED_QUERY_MAX_BYTES: usize = TASK_QUERY_MAX_BYTES - EVENT_QUERY_MAX_BYTES;
const QUERY_VALUE_MAX_FRAGMENTS: usize = 64;
const EVENT_IDENTIFIER_MAX_VALUES: usize = 4;
const EVENT_IDENTIFIER_MAX_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryOrigin {
    AuthoredTask,
    AuthoredTemplate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryFragment {
    pub(crate) origin: QueryOrigin,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct QueryValue {
    pub(crate) fragments: Vec<QueryFragment>,
}

impl QueryValue {
    fn authored(origin: QueryOrigin, text: &str) -> Self {
        let mut value = Self::default();
        value.push_fragment(origin, text);
        value
    }

    fn append(&mut self, other: &Self) {
        for fragment in &other.fragments {
            self.push_fragment(fragment.origin, &fragment.text);
        }
    }

    fn retained_bytes(&self) -> usize {
        self.fragments
            .iter()
            .map(|fragment| fragment.text.len())
            .sum::<usize>()
            + self.fragments.len().saturating_sub(1)
    }

    fn push_fragment(&mut self, origin: QueryOrigin, text: &str) {
        if self.fragments.len() >= QUERY_VALUE_MAX_FRAGMENTS {
            return;
        }
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let separator_bytes = usize::from(!self.fragments.is_empty());
        let remaining = AUTHORED_QUERY_MAX_BYTES.saturating_sub(self.retained_bytes());
        if remaining <= separator_bytes {
            return;
        }
        let text = utf8_prefix(text, remaining - separator_bytes).trim_end();
        if !text.is_empty() {
            self.fragments.push(QueryFragment {
                origin,
                text: text.to_owned(),
            });
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EventQueryValue {
    identifiers: String,
    text: String,
}

/// Query-only execution state. It preserves eligible origins without parsing
/// the provider prompt, which also contains memory, identity, tool output,
/// trigger wrappers, and trust headings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TaskQueryState {
    pub(crate) current_context: QueryValue,
    pub(crate) last_output: QueryValue,
    event: EventQueryValue,
}

impl TaskQueryState {
    pub(crate) fn replace_with_authored_input(&mut self, prompt: &str) {
        self.current_context = QueryValue::authored(QueryOrigin::AuthoredTask, prompt);
    }

    pub(crate) fn clear_last_output(&mut self) {
        self.last_output = QueryValue::default();
    }

    pub(crate) fn apply_authored_template(
        &mut self,
        template: &str,
        mode: &str,
        rendered_context_was_empty: bool,
    ) {
        let rendered = render_authored_template(template, &self.current_context, &self.last_output);

        if mode == "replace" || rendered_context_was_empty {
            self.current_context = rendered.clone();
        } else {
            self.current_context.append(&rendered);
        }
        self.last_output = rendered;
    }

    pub(crate) fn set_normalized_event(&mut self, payload: &str, is_app_event: bool) {
        self.event = if is_app_event {
            event_query_text(payload)
        } else {
            EventQueryValue::default()
        };
    }

    pub(crate) fn query_text(&self, rendered_context_is_empty: bool) -> String {
        let selected = if rendered_context_is_empty {
            &self.last_output
        } else {
            &self.current_context
        };
        let mut output = String::new();
        // Stable event identifiers get a bounded allocation at the front so
        // the query builder's global term ceiling cannot discard them after a
        // long authored task. Authored/template fragments keep their own order.
        append_bounded(&mut output, &self.event.identifiers, EVENT_QUERY_MAX_BYTES);
        for fragment in &selected.fragments {
            append_bounded(
                &mut output,
                &fragment.text,
                EVENT_QUERY_MAX_BYTES + AUTHORED_QUERY_MAX_BYTES,
            );
        }
        append_bounded(&mut output, &self.event.text, TASK_QUERY_MAX_BYTES);
        output
    }
}

fn render_authored_template(
    template: &str,
    current_context: &QueryValue,
    last_output: &QueryValue,
) -> QueryValue {
    const PLACEHOLDERS: &[(&str, Option<bool>)] = &[
        ("{{context}}", Some(true)),
        ("{{context_prompt}}", Some(true)),
        ("{{output}}", Some(false)),
        ("{{last_output}}", Some(false)),
        ("{{cwd}}", None),
    ];

    let mut rendered = QueryValue::default();
    let mut remainder = template;
    loop {
        let next = PLACEHOLDERS
            .iter()
            .filter_map(|(placeholder, source)| {
                remainder
                    .find(placeholder)
                    .map(|index| (index, *placeholder, *source))
            })
            .min_by_key(|(index, _, _)| *index);
        let Some((index, placeholder, source)) = next else {
            rendered.append(&QueryValue::authored(
                QueryOrigin::AuthoredTemplate,
                remainder,
            ));
            break;
        };
        rendered.append(&QueryValue::authored(
            QueryOrigin::AuthoredTemplate,
            &remainder[..index],
        ));
        match source {
            Some(true) => rendered.append(current_context),
            Some(false) => rendered.append(last_output),
            None => {}
        }
        remainder = &remainder[index + placeholder.len()..];
    }
    rendered
}

const EVENT_TEXT_KEYS: &[&str] = &["message", "preview", "subject"];
const EVENT_IDENTIFIER_KEYS: &[&str] = &[
    "documentId",
    "document_id",
    "identifier",
    "issueId",
    "issue_id",
    "issueIdentifier",
    "messageId",
    "message_id",
    "projectId",
    "project_id",
    "shortId",
    "short_id",
    "ticketId",
    "ticket_id",
];

fn event_query_text(payload: &str) -> EventQueryValue {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return EventQueryValue::default();
    };
    let mut identifiers = Vec::new();
    let mut text = Vec::new();
    collect_event_leaves(&value, &mut identifiers, &mut text);

    let mut identifier_output = String::new();
    for identifier in identifiers.into_iter().take(EVENT_IDENTIFIER_MAX_VALUES) {
        let identifier = utf8_prefix(identifier.trim(), EVENT_IDENTIFIER_MAX_BYTES);
        append_bounded(
            &mut identifier_output,
            identifier,
            EVENT_IDENTIFIER_MAX_VALUES * (EVENT_IDENTIFIER_MAX_BYTES + 1),
        );
    }
    let separator_bytes = usize::from(!identifier_output.is_empty() && !text.is_empty());
    let text_budget = EVENT_QUERY_MAX_BYTES
        .saturating_sub(identifier_output.len())
        .saturating_sub(separator_bytes);
    let mut text_output = String::new();
    for leaf in text {
        append_bounded(&mut text_output, leaf, text_budget);
    }
    EventQueryValue {
        identifiers: identifier_output,
        text: text_output,
    }
}

fn collect_event_leaves<'a>(
    value: &'a Value,
    identifiers: &mut Vec<&'a str>,
    text: &mut Vec<&'a str>,
) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if EVENT_IDENTIFIER_KEYS.contains(&key.as_str()) {
                    if let Some(text) = value.as_str().filter(|text| !text.trim().is_empty()) {
                        identifiers.push(text);
                    }
                } else if EVENT_TEXT_KEYS.contains(&key.as_str()) {
                    if let Some(value) = value.as_str().filter(|value| !value.trim().is_empty()) {
                        text.push(value);
                    }
                } else if key == "attributes" {
                    collect_event_leaves(value, identifiers, text);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_event_leaves(item, identifiers, text);
            }
        }
        _ => {}
    }
}

fn utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn append_bounded(output: &mut String, value: &str, max_bytes: usize) {
    if output.len() >= max_bytes || value.trim().is_empty() {
        return;
    }
    if !output.is_empty() {
        output.push('\n');
    }
    for character in value.trim().chars() {
        if output.len() + character.len_utf8() > max_bytes {
            break;
        }
        output.push(character);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_replaces_current_task_and_generated_output_is_not_a_query_source() {
        let mut state = TaskQueryState::default();
        state.replace_with_authored_input("first task");
        state.replace_with_authored_input("release checklist");
        state.clear_last_output();
        assert_eq!(state.query_text(false), "release checklist");
        assert_eq!(state.query_text(true), "");
    }

    #[test]
    fn template_append_replace_and_placeholders_preserve_origins() {
        let mut state = TaskQueryState::default();
        state.replace_with_authored_input("customer export");
        state.apply_authored_template("Check {{context}} in {{cwd}}", "append", false);
        let appended = state.query_text(false);
        assert!(appended.contains("customer export"));
        assert!(appended.contains("Check"));
        assert!(!appended.contains("{{context}}"));
        assert!(!appended.contains("{{cwd}}"));

        state.apply_authored_template("Deploy {{output}}", "replace", false);
        let replaced = state.query_text(false);
        assert!(replaced.contains("Deploy"));
        assert!(replaced.contains("Check"));
    }

    #[test]
    fn template_aliases_preserve_sources_in_placeholder_order() {
        let mut state = TaskQueryState::default();
        state.current_context = QueryValue::authored(QueryOrigin::AuthoredTask, "CURRENT");
        state.last_output = QueryValue::authored(QueryOrigin::AuthoredTemplate, "LAST");
        state.apply_authored_template(
            "before {{last_output}} middle {{context_prompt}} after",
            "replace",
            false,
        );
        assert_eq!(
            state.query_text(false),
            "before\nLAST\nmiddle\nCURRENT\nafter"
        );
        assert!(!state.query_text(false).contains("{{"));
    }

    #[test]
    fn template_order_controls_which_authored_terms_fit_the_budget() {
        let mut state = TaskQueryState::default();
        state.current_context = QueryValue::authored(QueryOrigin::AuthoredTask, &"c".repeat(5_000));
        state.last_output = QueryValue::authored(QueryOrigin::AuthoredTemplate, &"o".repeat(5_000));
        state.apply_authored_template("{{output}} {{context}} tail", "replace", false);
        let query = state.query_text(false);
        assert!(query.starts_with(&"o".repeat(5_000)));
        assert!(!query.contains("tail"));
        assert_eq!(query.len(), AUTHORED_QUERY_MAX_BYTES);
    }

    #[test]
    fn repeated_context_expansion_bounds_retained_state() {
        let mut state = TaskQueryState::default();
        state.replace_with_authored_input("seed");
        for _ in 0..100 {
            state.apply_authored_template(
                "prefix {{context}} middle {{context_prompt}} suffix",
                "replace",
                false,
            );
            assert!(state.current_context.retained_bytes() <= AUTHORED_QUERY_MAX_BYTES);
            assert!(state.current_context.fragments.len() <= QUERY_VALUE_MAX_FRAGMENTS);
            assert!(state.last_output.retained_bytes() <= AUTHORED_QUERY_MAX_BYTES);
            assert!(state.last_output.fragments.len() <= QUERY_VALUE_MAX_FRAGMENTS);
        }
        assert_eq!(
            state.current_context.fragments.len(),
            QUERY_VALUE_MAX_FRAGMENTS
        );
        state.apply_authored_template(
            "prefix {{context}} middle {{context_prompt}} suffix",
            "replace",
            false,
        );
        assert!(state.current_context.retained_bytes() <= AUTHORED_QUERY_MAX_BYTES);
        assert_eq!(
            state.current_context.fragments.len(),
            QUERY_VALUE_MAX_FRAGMENTS
        );
        assert_eq!(
            state.query_text(false).len(),
            state.current_context.retained_bytes()
        );
    }

    #[test]
    fn event_identifier_keeps_a_term_slot_after_long_authored_input() {
        use crate::db::{CreateWorkflowInput, MemoryRetrievalRequest, RetrievalPurpose};

        let db = crate::db::Db::open_in_memory().expect("open database");
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Event recall".into(),
                description: String::new(),
                working_directory: "/projects/alfred".into(),
                folder_id: None,
                graph: serde_json::json!({ "nodes": [], "edges": [] }),
            })
            .expect("create workflow");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO memories
                   (id, workflow_id, scope_type, scope_key, memory_type, source,
                    title, body, confidence, salience, status, created_at, updated_at)
                 VALUES ('event-memory', ?1, 'workflow', ?1, 'fact', 'manual',
                         'Ticket ALFRED-4821', 'Use the event-only release procedure',
                         1.0, 50, 'active', '2026-09-09T00:00:00Z',
                         '2026-09-09T00:00:00Z')",
                rusqlite::params![workflow.id],
            )?;
            crate::db::index_memory(conn, "event-memory")?;
            Ok(())
        })
        .expect("insert event memory");

        let authored = (0..25)
            .map(|index| format!("authoredterm{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut state = TaskQueryState::default();
        state.replace_with_authored_input(&authored);
        state.set_normalized_event(
            r#"{"subject":"ordinary notification", "attributes":{"projectId":"ALFRED-4821"}}"#,
            true,
        );
        let query_text = state.query_text(false);
        assert!(query_text.starts_with("ALFRED-4821\n"));

        let result = db.retrieve_memories(&MemoryRetrievalRequest {
            workflow_id: &workflow.id,
            working_directory: Some("/projects/alfred"),
            run_id: "event-run",
            node_id: "agent",
            query_text: &query_text,
            exclude_ids: &[],
            purpose: RetrievalPurpose::Automatic,
        });
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.memory_id.as_str())
                .collect::<Vec<_>>(),
            ["event-memory"]
        );
    }

    #[test]
    fn memory_to_output_template_contributes_no_memory_body() {
        let mut state = TaskQueryState::default();
        state.apply_authored_template("{{output}}", "replace", false);
        assert_eq!(state.query_text(false), "");
    }

    #[test]
    fn event_allowlist_keeps_text_and_stable_identifiers_only() {
        let mut state = TaskQueryState::default();
        state.set_normalized_event(
            r#"{"subject":"Release failed","preview":"ERR_MEMORY_017","actor":"Ada","attributes":{"projectId":"ALFRED-4821","token":"secret","random":"ignore"}}"#,
            true,
        );
        let query = state.query_text(true);
        assert!(query.contains("Release failed"));
        assert!(query.contains("ERR_MEMORY_017"));
        assert!(query.contains("ALFRED-4821"));
        assert!(!query.contains("Ada"));
        assert!(!query.contains("secret"));
        assert!(!query.contains("ignore"));
    }

    #[test]
    fn event_only_query_is_bounded_and_non_app_payload_is_absent() {
        let mut state = TaskQueryState::default();
        state.set_normalized_event(
            &serde_json::json!({"subject": "é".repeat(2_000)}).to_string(),
            true,
        );
        let query = state.query_text(true);
        assert!(query.len() <= EVENT_QUERY_MAX_BYTES);
        assert!(std::str::from_utf8(query.as_bytes()).is_ok());

        state.set_normalized_event(r#"{"subject":"manual wrapper"}"#, false);
        assert!(state.query_text(true).is_empty());
    }

    #[test]
    fn authored_and_event_allocations_fit_total_budget() {
        let mut state = TaskQueryState::default();
        state.replace_with_authored_input(&"task".repeat(3_000));
        state.set_normalized_event(
            &serde_json::json!({"message": "event".repeat(1_000)}).to_string(),
            true,
        );
        let query = state.query_text(false);
        assert!(query.len() <= TASK_QUERY_MAX_BYTES);
        assert!(query.contains("event"));
    }
}
