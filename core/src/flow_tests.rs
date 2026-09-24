//! Unit tests for `flow`.
use super::*;

const SIMPLE_MMD: &str = r"
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
";

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
    let mmd = r"
stateDiagram-v2
    [*] --> clean_code
    clean_code --> test_coverage
    clean_code --> skip_reason
    test_coverage --> [*]
    skip_reason   --> [*]
    clean_code    : Did you write clean, readable code?
    test_coverage : Did you increase test coverage?
    skip_reason   : Describe why test coverage was skipped.
";
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
    let mmd = r"stateDiagram-v2
    direction LR
    %% this is a comment
    [*] --> step
    step --> [*]
    step : Do it
";
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
    // "leaf" is a terminal state (only → [*]), so transitions.get("leaf")
    // returns only the pseudo-entry for [*], and next_states("leaf") is empty.
    let mmd = "stateDiagram-v2\n    [*] --> root\n    root --> leaf\n    leaf --> [*]\n    root : Root\n    leaf : Leaf\n";
    let g = parse_mmd("test.mmd", mmd).unwrap();
    assert!(g.order.contains(&"leaf".to_owned()));
    assert!(g.next_states("leaf").is_empty());
}

#[test]
fn error_on_cycle_with_no_exit() {
    // step_a and step_b form a cycle; neither reaches [*]
    let mmd = "stateDiagram-v2\n    [*] --> step_a\n    step_a --> step_b\n    step_b --> step_a\n";
    let err = parse_mmd("test.mmd", mmd).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("no path to [*]"), "got: {msg}");
}

#[test]
fn error_on_dead_end_state() {
    // step_b has no outgoing transition — it can never reach [*]
    let mmd = "stateDiagram-v2\n    [*] --> step_a\n    step_a --> step_b\n    step_a --> [*]\n";
    let err = parse_mmd("test.mmd", mmd).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("no path to [*]"), "got: {msg}");
}

#[test]
fn cycle_with_exit_is_valid() {
    // step_a has both a cycle to step_b AND a path to [*] — valid
    let mmd = "stateDiagram-v2\n    [*] --> step_a\n    step_a --> step_b\n    step_b --> step_a\n    step_a --> [*]\n";
    // step_b still has no path to [*] (only step_a does, and step_b can reach step_a)
    // Actually step_b → step_a → [*] IS a path, so this should be valid.
    // step_b can reach [*] via step_b → step_a → [*]
    let g = parse_mmd("test.mmd", mmd).unwrap();
    assert!(g.terminal.contains("step_a"));
}

#[test]
fn topo_order_deduplicates_via_visited() {
    // State referenced from multiple predecessors only appears once in order
    let mmd = "stateDiagram-v2\n    [*] --> a\n    [*] --> b\n    a --> c\n    b --> c\n    c --> [*]\n    a:A\n    b:B\n    c:C\n";
    let g = parse_mmd("test.mmd", mmd).unwrap();
    let count = g.order.iter().filter(|s| s.as_str() == "c").count();
    assert_eq!(count, 1);
}
