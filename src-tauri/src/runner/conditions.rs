//! Conditional connections: which graph edges fire, and therefore which steps
//! run (Plan 053).
//!
//! The whole feature is three rules, implemented here and in the runner's node
//! loop:
//!
//! 1. An edge fires when its source step produced a result and its condition is
//!    absent or evaluates true.
//! 2. A step runs when it has no incoming edges, or at least one incoming edge
//!    fired. Several firing edges still run it once.
//! 3. A step is skipped when it has incoming edges and none fired. A skipped
//!    step produces no result, so its own edges cannot fire — skipping
//!    propagates down the branch without a separate subtree walk.
//!
//! Everything in this module is pure: no database, no app handle, no I/O.

use std::collections::{HashMap, HashSet};

use regex::RegexBuilder;
use serde_json::Value;

/// Matches `MAX_EDGE_CONDITION_VALUE_BYTES` in `src/features/workflow/types.ts`.
pub const MAX_CONDITION_VALUE_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionSource {
    /// Text output of this edge's own source step.
    Output,
    /// Accumulated run context at evaluation time.
    Context,
    /// The run's trigger payload, or an empty string when there is none.
    Trigger,
}

impl ConditionSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "output" => Some(Self::Output),
            "context" => Some(Self::Context),
            "trigger" => Some(Self::Trigger),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionOperator {
    Contains,
    NotContains,
    Equals,
    NotEquals,
    Matches,
    IsEmpty,
    IsNotEmpty,
}

impl ConditionOperator {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "contains" => Some(Self::Contains),
            "notContains" => Some(Self::NotContains),
            "equals" => Some(Self::Equals),
            "notEquals" => Some(Self::NotEquals),
            "matches" => Some(Self::Matches),
            "isEmpty" => Some(Self::IsEmpty),
            "isNotEmpty" => Some(Self::IsNotEmpty),
            _ => None,
        }
    }

    /// Operators that ignore the comparison value.
    pub fn is_unary(self) -> bool {
        matches!(self, Self::IsEmpty | Self::IsNotEmpty)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::NotContains => "does not contain",
            Self::Equals => "equals",
            Self::NotEquals => "does not equal",
            Self::Matches => "matches regex",
            Self::IsEmpty => "is empty",
            Self::IsNotEmpty => "is not empty",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeCondition {
    pub source: ConditionSource,
    pub operator: ConditionOperator,
    pub value: String,
    pub case_sensitive: bool,
}

impl EdgeCondition {
    /// Read a condition off untrusted graph JSON. Anything that is not a fully
    /// valid condition reads as `None`, i.e. an unconditional edge — the same
    /// result as a graph written before this feature existed.
    pub fn parse(edge: &Value) -> Option<Self> {
        let raw = edge.get("data")?.get("condition")?;
        let source = ConditionSource::parse(raw.get("source")?.as_str()?)?;
        let operator = ConditionOperator::parse(raw.get("operator")?.as_str()?)?;
        let mut value = raw
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if value.len() > MAX_CONDITION_VALUE_BYTES {
            // Cut on a char boundary; a truncated pattern still evaluates or
            // fails closed rather than panicking.
            let mut end = MAX_CONDITION_VALUE_BYTES;
            while end > 0 && !value.is_char_boundary(end) {
                end -= 1;
            }
            value.truncate(end);
        }
        Some(Self {
            source,
            operator,
            value,
            case_sensitive: raw
                .get("caseSensitive")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// Chip-style text for run events and skipped-step notes.
    pub fn summary(&self) -> String {
        let subject = match self.source {
            ConditionSource::Output => "step output",
            ConditionSource::Context => "run context",
            ConditionSource::Trigger => "trigger payload",
        };
        if self.operator.is_unary() {
            format!("{subject} {}", self.operator.label())
        } else {
            format!("{subject} {} \"{}\"", self.operator.label(), self.value)
        }
    }
}

/// Compile a user-authored pattern exactly as the runner will evaluate it.
///
/// `multi_line` is on because the subject is almost always a multi-line step
/// output: without it `^FAIL` only matches when the whole output starts with
/// FAIL, so a line anchor silently never fires. `.` still stops at a newline,
/// matching what people expect from a line-oriented search.
pub fn compile_pattern(pattern: &str, case_sensitive: bool) -> Result<regex::Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .multi_line(true)
        .size_limit(1 << 20)
        .build()
}

/// Values a condition can compare against, gathered once per evaluation.
pub struct ConditionSubjects<'a> {
    pub output: &'a str,
    pub context: &'a str,
    pub trigger: &'a str,
}

/// Evaluate one condition. An uncompilable regex is false, never an error: a
/// bad pattern must not end an otherwise healthy run.
pub fn evaluate_condition(condition: &EdgeCondition, subjects: &ConditionSubjects<'_>) -> bool {
    let subject = match condition.source {
        ConditionSource::Output => subjects.output,
        ConditionSource::Context => subjects.context,
        ConditionSource::Trigger => subjects.trigger,
    };

    match condition.operator {
        ConditionOperator::IsEmpty => return subject.trim().is_empty(),
        ConditionOperator::IsNotEmpty => return !subject.trim().is_empty(),
        ConditionOperator::Matches => {
            return compile_pattern(&condition.value, condition.case_sensitive)
                .map(|re| re.is_match(subject))
                .unwrap_or(false);
        }
        _ => {}
    }

    // Text operators. Case-insensitive comparison lowercases both sides; this
    // is ASCII-simple folding, which is what users expect from a "contains" box.
    let (haystack, needle) = if condition.case_sensitive {
        (subject.to_string(), condition.value.clone())
    } else {
        (subject.to_lowercase(), condition.value.to_lowercase())
    };

    match condition.operator {
        ConditionOperator::Contains => haystack.contains(&needle),
        ConditionOperator::NotContains => !haystack.contains(&needle),
        ConditionOperator::Equals => haystack.trim() == needle.trim(),
        ConditionOperator::NotEquals => haystack.trim() != needle.trim(),
        ConditionOperator::Matches
        | ConditionOperator::IsEmpty
        | ConditionOperator::IsNotEmpty => unreachable!("handled above"),
    }
}

/// Rule 1. `source_output` is `None` when the source step never produced a
/// result — it was skipped, or has not run — which makes the edge dead.
pub fn edge_fires(
    edge: &Value,
    source_output: Option<&str>,
    context: &str,
    trigger: &str,
) -> bool {
    let Some(output) = source_output else {
        return false;
    };
    match EdgeCondition::parse(edge) {
        None => true,
        Some(condition) => evaluate_condition(
            &condition,
            &ConditionSubjects {
                output,
                context,
                trigger,
            },
        ),
    }
}

/// Incoming edges per target node, in graph order. Edges referencing a node
/// that is not in the graph are dropped, matching `topological_order`.
pub fn incoming_edges(nodes: &[Value], edges: &[Value]) -> HashMap<String, Vec<Value>> {
    let ids: HashSet<&str> = nodes
        .iter()
        .filter_map(|node| node.get("id").and_then(Value::as_str))
        .collect();

    let mut incoming: HashMap<String, Vec<Value>> = HashMap::new();
    for edge in edges {
        let source = edge.get("source").and_then(Value::as_str);
        let target = edge.get("target").and_then(Value::as_str);
        if let (Some(source), Some(target)) = (source, target) {
            if ids.contains(source) && ids.contains(target) {
                incoming.entry(target.to_string()).or_default().push(edge.clone());
            }
        }
    }
    incoming
}

/// Why a step is not running. `Some` is the reason to record on the skipped
/// step; `None` means the step runs.
pub struct SkipReason(pub String);

/// Rules 2 and 3. Returns `None` when the step should run.
pub fn skip_reason(
    node_id: &str,
    incoming: &HashMap<String, Vec<Value>>,
    node_outputs: &HashMap<String, String>,
    context: &str,
    trigger: &str,
) -> Option<SkipReason> {
    let edges = incoming.get(node_id)?;
    if edges.is_empty() {
        // A root, or every incoming edge pointed at a missing node.
        return None;
    }

    let mut unmet: Vec<String> = Vec::new();
    for edge in edges {
        let source = edge.get("source").and_then(Value::as_str).unwrap_or_default();
        let source_output = node_outputs.get(source).map(String::as_str);
        if edge_fires(edge, source_output, context, trigger) {
            return None;
        }
        unmet.push(match EdgeCondition::parse(edge) {
            Some(condition) if source_output.is_some() => {
                format!("{source}: {}", condition.summary())
            }
            _ => format!("{source}: did not run"),
        });
    }

    Some(SkipReason(format!(
        "Skipped — no incoming connection fired ({})",
        unmet.join("; ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn subjects<'a>(output: &'a str) -> ConditionSubjects<'a> {
        ConditionSubjects {
            output,
            context: "",
            trigger: "",
        }
    }

    fn condition(operator: &str, value: &str) -> EdgeCondition {
        EdgeCondition {
            source: ConditionSource::Output,
            operator: ConditionOperator::parse(operator).expect("operator"),
            value: value.into(),
            case_sensitive: false,
        }
    }

    #[test]
    fn text_operators_cover_both_directions() {
        assert!(evaluate_condition(
            &condition("contains", "fail"),
            &subjects("Tests FAILED")
        ));
        assert!(!evaluate_condition(
            &condition("notContains", "fail"),
            &subjects("Tests FAILED")
        ));
        assert!(evaluate_condition(
            &condition("equals", "done"),
            &subjects("  DONE  ")
        ));
        assert!(evaluate_condition(
            &condition("notEquals", "done"),
            &subjects("not done")
        ));
    }

    #[test]
    fn case_sensitivity_is_respected() {
        let mut sensitive = condition("contains", "FAIL");
        sensitive.case_sensitive = true;
        assert!(evaluate_condition(&sensitive, &subjects("a FAIL here")));
        assert!(!evaluate_condition(&sensitive, &subjects("a fail here")));
        assert!(evaluate_condition(
            &condition("contains", "FAIL"),
            &subjects("a fail here")
        ));
    }

    #[test]
    fn empty_operators_ignore_the_value_and_whitespace() {
        assert!(evaluate_condition(
            &condition("isEmpty", "ignored"),
            &subjects("   \n ")
        ));
        assert!(!evaluate_condition(&condition("isEmpty", ""), &subjects("x")));
        assert!(evaluate_condition(
            &condition("isNotEmpty", ""),
            &subjects("x")
        ));
    }

    #[test]
    fn line_anchors_match_line_starts_in_multi_line_output() {
        // Step output is nearly always multi-line. Without multi_line this is
        // false and the branch silently never runs.
        assert!(evaluate_condition(
            &condition("matches", "^FAIL"),
            &subjects("tests ran\nFAIL: 3 suites")
        ));
        assert!(evaluate_condition(
            &condition("matches", "suites$"),
            &subjects("FAIL: 3 suites\nsee log")
        ));
    }

    #[test]
    fn a_dot_still_stops_at_a_line_break() {
        // Line-oriented search: `.` spanning lines would surprise.
        assert!(!evaluate_condition(
            &condition("matches", "FAIL.*ERROR"),
            &subjects("FAIL\nERROR")
        ));
    }

    #[test]
    fn look_around_is_unsupported_and_must_be_reported_not_silently_false() {
        // The Rust engine has no look-around. The editor must surface this
        // instead of saving a pattern that can never fire.
        assert!(compile_pattern("FAIL(?!ED)", false).is_err());
        assert!(compile_pattern("(?=FAIL)", false).is_err());
        // Inline flags are supported here but rejected by JavaScript's engine.
        assert!(compile_pattern("(?i)fail", true).is_ok());
    }

    #[test]
    fn regex_matches_and_bad_patterns_are_false() {
        assert!(evaluate_condition(
            &condition("matches", "FAIL|error"),
            &subjects("an error occurred")
        ));
        // Unbalanced group: must be false, never a panic or a run failure.
        assert!(!evaluate_condition(
            &condition("matches", "FAIL("),
            &subjects("FAIL(")
        ));
    }

    #[test]
    fn each_source_reads_its_own_subject() {
        let subjects = ConditionSubjects {
            output: "out",
            context: "ctx",
            trigger: "trg",
        };
        for (source, hit) in [
            (ConditionSource::Output, "out"),
            (ConditionSource::Context, "ctx"),
            (ConditionSource::Trigger, "trg"),
        ] {
            let condition = EdgeCondition {
                source,
                operator: ConditionOperator::Equals,
                value: hit.into(),
                case_sensitive: false,
            };
            assert!(evaluate_condition(&condition, &subjects), "{hit}");
        }
    }

    #[test]
    fn missing_or_malformed_conditions_read_as_unconditional() {
        assert!(EdgeCondition::parse(&json!({"source": "a", "target": "b"})).is_none());
        assert!(EdgeCondition::parse(&json!({"data": {}})).is_none());
        assert!(EdgeCondition::parse(
            &json!({"data": {"condition": {"source": "nope", "operator": "contains"}}})
        )
        .is_none());
        assert!(EdgeCondition::parse(
            &json!({"data": {"condition": {"source": "output", "operator": "nope"}}})
        )
        .is_none());
    }

    #[test]
    fn oversized_values_are_truncated_on_a_char_boundary() {
        let long = "é".repeat(MAX_CONDITION_VALUE_BYTES);
        let parsed = EdgeCondition::parse(
            &json!({"data": {"condition": {"source": "output", "operator": "contains", "value": long}}}),
        )
        .expect("condition");
        assert!(parsed.value.len() <= MAX_CONDITION_VALUE_BYTES);
        assert!(parsed.value.chars().all(|c| c == 'é'));
    }

    #[test]
    fn an_edge_from_a_step_that_never_ran_is_dead() {
        let edge = json!({"source": "a", "target": "b"});
        assert!(!edge_fires(&edge, None, "", ""));
        assert!(edge_fires(&edge, Some(""), "", ""));
    }

    fn node(id: &str) -> Value {
        json!({"id": id, "type": "agent", "data": {}})
    }

    fn conditional_edge(source: &str, target: &str, needle: &str) -> Value {
        json!({
            "id": format!("{source}-{target}"),
            "source": source,
            "target": target,
            "data": {"condition": {
                "source": "output",
                "operator": "contains",
                "value": needle,
            }},
        })
    }

    #[test]
    fn a_root_always_runs() {
        let nodes = vec![node("a"), node("b")];
        let edges = vec![json!({"id": "e", "source": "a", "target": "b"})];
        let incoming = incoming_edges(&nodes, &edges);
        assert!(skip_reason("a", &incoming, &HashMap::new(), "", "").is_none());
    }

    #[test]
    fn one_firing_edge_is_enough_at_a_join() {
        let nodes = vec![node("a"), node("b"), node("join")];
        let edges = vec![
            conditional_edge("a", "join", "yes"),
            conditional_edge("b", "join", "yes"),
        ];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), "yes".to_string());
        outputs.insert("b".to_string(), "no".to_string());
        assert!(skip_reason("join", &incoming, &outputs, "", "").is_none());
    }

    #[test]
    fn a_step_with_no_firing_edge_is_skipped_with_a_reason() {
        let nodes = vec![node("a"), node("b")];
        let edges = vec![conditional_edge("a", "b", "yes")];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), "no".to_string());
        let reason = skip_reason("b", &incoming, &outputs, "", "").expect("skipped");
        assert!(reason.0.contains("step output contains"));
        assert!(reason.0.starts_with("Skipped"));
    }

    #[test]
    fn skipping_propagates_because_a_skipped_step_has_no_output() {
        let nodes = vec![node("a"), node("b"), node("c")];
        let edges = vec![
            conditional_edge("a", "b", "yes"),
            json!({"id": "bc", "source": "b", "target": "c"}),
        ];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), "no".to_string());

        assert!(skip_reason("b", &incoming, &outputs, "", "").is_some());
        // `b` recorded no output, so the unconditional `b → c` edge is dead too.
        let reason = skip_reason("c", &incoming, &outputs, "", "").expect("skipped");
        assert!(reason.0.contains("did not run"));
    }

    #[test]
    fn a_graph_with_no_conditions_never_skips_a_reached_step() {
        let nodes = vec![node("a"), node("b"), node("c")];
        let edges = vec![
            json!({"id": "ab", "source": "a", "target": "b"}),
            json!({"id": "bc", "source": "b", "target": "c"}),
        ];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), String::new());
        assert!(skip_reason("b", &incoming, &outputs, "", "").is_none());
        outputs.insert("b".to_string(), String::new());
        assert!(skip_reason("c", &incoming, &outputs, "", "").is_none());
    }

    #[test]
    fn edges_pointing_at_missing_nodes_are_dropped() {
        let nodes = vec![node("a")];
        let edges = vec![json!({"id": "e", "source": "ghost", "target": "a"})];
        let incoming = incoming_edges(&nodes, &edges);
        assert!(incoming.is_empty());
        assert!(skip_reason("a", &incoming, &HashMap::new(), "", "").is_none());
    }
}
