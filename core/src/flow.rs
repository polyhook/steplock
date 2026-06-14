use std::collections::{HashMap, HashSet};

use crate::error::{Result, SteplockError};

#[derive(Debug, Clone)]
pub struct FlowGraph {
    /// States reachable from `[*]` (the initial states).
    pub initial: Vec<String>,
    /// Outgoing transitions per state. States that go to `[*]` map to `vec!["[*]"]`.
    pub transitions: HashMap<String, Vec<String>>,
    /// Human-readable label per state.
    pub labels: HashMap<String, String>,
    /// States with a transition to `[*]` (terminal states).
    pub terminal: HashSet<String>,
    /// Topological order of non-pseudo states (for preview output).
    pub order: Vec<String>,
}

impl FlowGraph {
    /// Returns states that are not yet visited and not the pseudo `[*]` node.
    pub fn pending_after(&self, visited: &[String]) -> Vec<String> {
        let visited_set: HashSet<&str> = visited.iter().map(|s| s.as_str()).collect();
        self.order
            .iter()
            .filter(|s| !visited_set.contains(s.as_str()))
            .cloned()
            .collect()
    }

    /// Outgoing transitions from `state`, excluding `[*]`.
    pub fn next_states(&self, state: &str) -> Vec<String> {
        self.transitions
            .get(state)
            .map(|v| v.iter().filter(|s| s.as_str() != "[*]").cloned().collect())
            .unwrap_or_default()
    }

    /// True if `state` is a terminal state (transitions to `[*]`).
    pub fn is_terminal(&self, state: &str) -> bool {
        self.terminal.contains(state)
    }
}

/// Parse a Mermaid `stateDiagram-v2` diagram into a `FlowGraph`.
///
/// # Errors
///
/// Returns `Err` if `content` contains no `[*] --> <state>` initial transition.
pub fn parse_mmd(path: &str, content: &str) -> Result<FlowGraph> {
    let mut transitions: HashMap<String, Vec<String>> = HashMap::new();
    let mut labels: HashMap<String, String> = HashMap::new();
    let mut initial: Vec<String> = Vec::new();
    let mut terminal: HashSet<String> = HashSet::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("%%") || line == "stateDiagram-v2" {
            continue;
        }

        if let Some(rest) = line.strip_prefix("direction ") {
            let _ = rest; // ignore direction hints
            continue;
        }

        // Transition: X --> Y
        if let Some((lhs, rhs)) = split_transition(line) {
            let lhs = lhs.trim().to_owned();
            let rhs = rhs.trim().to_owned();

            if lhs == "[*]" {
                // Initial transition
                if !initial.contains(&rhs) {
                    initial.push(rhs.clone());
                }
                // [*] is not stored as a real state
            } else if rhs == "[*]" {
                terminal.insert(lhs.clone());
                transitions.entry(lhs).or_default().push("[*]".to_owned());
            } else {
                transitions.entry(lhs).or_default().push(rhs);
            }
            continue;
        }

        // Label: state : Label text
        if let Some((state, label)) = split_label(line) {
            labels.insert(state.trim().to_owned(), label.trim().to_owned());
            continue;
        }
    }

    if initial.is_empty() {
        return Err(SteplockError::Mermaid {
            path: path.to_owned(),
            message: "no [*] --> <state> initial transition found".to_owned(),
        });
    }

    // Build topological order via BFS from initial states.
    let order = topo_order(&initial, &transitions);

    Ok(FlowGraph {
        initial,
        transitions,
        labels,
        terminal,
        order,
    })
}

fn split_transition(line: &str) -> Option<(&str, &str)> {
    let pos = line.find("-->")?;
    Some((&line[..pos], &line[pos + 3..]))
}

fn split_label(line: &str) -> Option<(&str, &str)> {
    // Called only when split_transition returned None (no "-->").
    let pos = line.find(':')?;
    Some((&line[..pos], &line[pos + 1..]))
}

fn topo_order(initial: &[String], transitions: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut queue: std::collections::VecDeque<String> = initial.iter().cloned().collect();

    while let Some(state) = queue.pop_front() {
        if visited.contains(&state) {
            continue;
        }
        visited.insert(state.clone());
        order.push(state.clone());
        if let Some(nexts) = transitions.get(&state) {
            for next in nexts {
                if next != "[*]" && !visited.contains(next) {
                    queue.push_back(next.clone());
                }
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_MMD: &str = r#"
stateDiagram-v2
    [*] --> clean_code
    clean_code --> test_coverage
    test_coverage --> documentation
    documentation --> no_secrets
    no_secrets --> [*]

    clean_code   : Did you write clean, readable code?
    test_coverage: Did you increase test coverage by at least a little?
    documentation: Did you update relevant documentation?
    no_secrets   : Did you check for hardcoded secrets or credentials?
"#;

    #[test]
    fn parses_simple_flow() {
        let g = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        assert_eq!(g.initial, vec!["clean_code"]);
        assert_eq!(
            g.order,
            vec!["clean_code", "test_coverage", "documentation", "no_secrets"]
        );
        assert!(g.is_terminal("no_secrets"));
        assert_eq!(g.next_states("clean_code"), vec!["test_coverage"]);
        assert_eq!(
            g.labels["clean_code"],
            "Did you write clean, readable code?"
        );
    }

    #[test]
    fn parses_branching_flow() {
        let mmd = r#"
stateDiagram-v2
    [*] --> clean_code
    clean_code --> test_coverage
    clean_code --> skip_reason
    test_coverage --> [*]
    skip_reason   --> [*]
    clean_code    : Did you write clean, readable code?
    test_coverage : Did you increase test coverage?
    skip_reason   : Describe why test coverage was skipped.
"#;
        let g = parse_mmd("test.mmd", mmd).unwrap();
        let mut nexts = g.next_states("clean_code");
        nexts.sort();
        assert_eq!(nexts, vec!["skip_reason", "test_coverage"]);
        assert!(g.is_terminal("test_coverage"));
        assert!(g.is_terminal("skip_reason"));
    }

    #[test]
    fn error_on_missing_initial_state() {
        let mmd = "stateDiagram-v2\n    a --> b\n";
        let err = parse_mmd("test.mmd", mmd);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("no [*]"));
    }

    #[test]
    fn ignores_direction_and_comments() {
        let mmd = r#"stateDiagram-v2
    direction LR
    %% this is a comment
    [*] --> step
    step --> [*]
    step : Do it
"#;
        let g = parse_mmd("test.mmd", mmd).unwrap();
        assert_eq!(g.initial, vec!["step"]);
        assert_eq!(g.labels["step"], "Do it");
    }

    #[test]
    fn pending_after_returns_unvisited() {
        let g = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        let pending = g.pending_after(&["clean_code".to_owned(), "test_coverage".to_owned()]);
        assert_eq!(pending, vec!["documentation", "no_secrets"]);
    }

    #[test]
    fn pending_after_all_visited_is_empty() {
        let g = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        let all: Vec<String> = g.order.clone();
        assert!(g.pending_after(&all).is_empty());
    }

    #[test]
    fn next_states_for_terminal_excludes_pseudo() {
        let g = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        // no_secrets is terminal; next_states should be empty (excludes [*])
        assert!(g.next_states("no_secrets").is_empty());
    }

    #[test]
    fn next_states_for_unknown_state_is_empty() {
        let g = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        assert!(g.next_states("nonexistent").is_empty());
    }

    #[test]
    fn is_terminal_false_for_non_terminal() {
        let g = parse_mmd("test.mmd", SIMPLE_MMD).unwrap();
        assert!(!g.is_terminal("clean_code"));
    }

    #[test]
    fn duplicate_initial_not_added_twice() {
        // Two [*] --> same_state transitions should not produce duplicates
        let mmd = "stateDiagram-v2\n    [*] --> s\n    [*] --> s\n    s --> [*]\n    s : Step\n";
        let g = parse_mmd("test.mmd", mmd).unwrap();
        assert_eq!(g.initial.len(), 1);
    }

    #[test]
    fn ignores_unlabeled_bare_state_lines() {
        // A line with no --> and no : is silently ignored
        let mmd = "stateDiagram-v2\n    [*] --> s\n    s --> [*]\n    s : Step\n    orphan_note\n";
        let g = parse_mmd("test.mmd", mmd).unwrap();
        assert_eq!(g.order, vec!["s"]);
    }

    #[test]
    fn state_with_no_outgoing_transitions_in_order() {
        // State appears in order but has no transitions entry (only a destination, no label/source)
        let mmd = "stateDiagram-v2\n    [*] --> a\n    a --> b\n    b --> c\n    c --> [*]\n    a : Step A\n    b : Step B\n    c : Step C\n";
        let g = parse_mmd("test.mmd", mmd).unwrap();
        // 'c' has a transition to [*] — still appears
        assert!(g.order.contains(&"c".to_owned()));
    }

    #[test]
    fn state_with_no_outgoing_transitions_included_in_order() {
        // "leaf" has no outgoing transitions → transitions.get("leaf") returns None
        let mmd = "stateDiagram-v2\n    [*] --> root\n    root --> leaf\n    root : Root\n    leaf : Leaf\n";
        let g = parse_mmd("test.mmd", mmd).unwrap();
        assert!(g.order.contains(&"leaf".to_owned()));
        assert!(g.next_states("leaf").is_empty());
    }

    #[test]
    fn topo_order_deduplicates_via_visited() {
        // State referenced from multiple predecessors only appears once in order
        let mmd = "stateDiagram-v2\n    [*] --> a\n    [*] --> b\n    a --> c\n    b --> c\n    c --> [*]\n    a:A\n    b:B\n    c:C\n";
        let g = parse_mmd("test.mmd", mmd).unwrap();
        let count = g.order.iter().filter(|s| s.as_str() == "c").count();
        assert_eq!(count, 1);
    }
}
