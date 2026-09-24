use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::{Result, SteplockError};

/// Parsed representation of a Mermaid `stateDiagram-v2` checklist flow.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
    #[must_use]
    pub fn pending_after(&self, visited: &[String]) -> Vec<String> {
        let visited_set: HashSet<&str> = visited.iter().map(String::as_str).collect();
        self.order
            .iter()
            .filter(|s| !visited_set.contains(s.as_str()))
            .cloned()
            .collect()
    }

    /// Outgoing transitions from `state`, excluding `[*]`.
    #[must_use]
    pub fn next_states(&self, state: &str) -> Vec<String> {
        self.transitions
            .get(state)
            .map(|v| v.iter().filter(|s| s.as_str() != "[*]").cloned().collect())
            .unwrap_or_default()
    }

    /// True if `state` is a terminal state (transitions to `[*]`).
    #[must_use]
    pub fn is_terminal(&self, state: &str) -> bool {
        self.terminal.contains(state)
    }
}

/// Parse a Mermaid `stateDiagram-v2` diagram into a [`FlowGraph`].
///
/// # Errors
///
/// Returns [`SteplockError::Mermaid`] if the diagram has no `[*] --> <state>` initial transition.
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

        if line.strip_prefix("direction ").is_some() {
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

    // Verify every reachable state has a path to [*].  A state without such a
    // path is either part of a cycle or a dead end — both cause the checklist
    // to block forever and should be caught at parse time.
    let can_finish = states_that_can_reach_terminal(&terminal, &transitions);
    for state in &order {
        if !can_finish.contains(state.as_str()) {
            return Err(SteplockError::Mermaid {
                path: path.to_owned(),
                message: format!(
                    "state '{state}' has no path to [*] — check for cycles or dead ends"
                ),
            });
        }
    }

    Ok(FlowGraph {
        initial,
        transitions,
        labels,
        terminal,
        order,
    })
}

fn split_transition(line: &str) -> Option<(&str, &str)> {
    line.split_once("-->")
}

fn split_label(line: &str) -> Option<(&str, &str)> {
    // Called only when split_transition returned None (no "-->").
    line.split_once(':')
}

/// Returns the set of state names that have at least one path to `[*]`.
/// Uses reverse BFS: start from terminal states and walk backwards.
fn states_that_can_reach_terminal<'a>(
    terminal: &'a HashSet<String>,
    transitions: &'a HashMap<String, Vec<String>>,
) -> HashSet<&'a str> {
    // Build reverse adjacency (target → sources).
    let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, tos) in transitions {
        for to in tos {
            if to != "[*]" {
                reverse.entry(to.as_str()).or_default().push(from.as_str());
            }
        }
    }

    let mut reachable: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = terminal.iter().map(String::as_str).collect();

    while let Some(state) = queue.pop_front() {
        if reachable.contains(state) {
            continue;
        }
        reachable.insert(state);
        if let Some(preds) = reverse.get(state) {
            for pred in preds {
                queue.push_back(pred);
            }
        }
    }
    reachable
}

fn topo_order(initial: &[String], transitions: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut queue: VecDeque<String> = initial.iter().cloned().collect();

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
#[allow(clippy::indexing_slicing, clippy::unwrap_used)]
#[path = "flow_tests.rs"]
mod tests;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "flow_proptest_tests.rs"]
mod proptest_tests;
