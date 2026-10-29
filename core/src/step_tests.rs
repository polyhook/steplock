//! Unit tests for `step`.
use super::*;
use crate::run::run;
use crate::state::HookResponse;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

const FLOW: &str = r"stateDiagram-v2
    [*] --> review
    review --> ship
    review --> abort
    ship --> [*]
    abort --> [*]
    review: Reviewed the diff?
    ship: Ship it
    abort: Abandon the change
";

fn event(command: &str) -> HookEvent {
    let mut input = HashMap::new();
    input.insert(
        "command".to_owned(),
        serde_json::Value::String(command.to_owned()),
    );
    HookEvent::new(
        "tool:before".to_owned(),
        "bash".to_owned(),
        input,
        HashMap::new(),
        "s1".to_owned(),
        "claude-code".to_owned(),
    )
}

/// Write a `gate` checklist that blocks `git push`, block once, return the steplock dir.
fn blocked(root: &Path) -> PathBuf {
    let dir = root.join(".steplock/checklists/gate");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.toml"),
        "on_event = \"tool:before\"\non_tool = \"bash\"\nmatch_input = \"input.command.contains('git push')\"\nreset = \"session\"\n",
    )
    .unwrap();
    fs::write(dir.join("flow.mmd"), FLOW).unwrap();
    let resp = run(&event("git push"), root).unwrap();
    assert!(
        matches!(resp, HookResponse::Block { .. }),
        "first push must block"
    );
    root.join(".steplock")
}

fn current(steplock_dir: &Path) -> String {
    load_state(&state_path(steplock_dir, "s1", "gate"))
        .unwrap()
        .current_state
}

#[test]
fn records_each_step_until_the_gate_approves() {
    let tmp = TempDir::new().unwrap();
    let dir = blocked(tmp.path());

    let next = record_step_done(&dir, "s1", "gate", "review", Some("ship"), "read diff").unwrap();
    assert_eq!(next, "ship", "branch outcome is the chosen state");
    let done = record_step_done(&dir, "s1", "gate", "ship", None, "pushed nothing yet").unwrap();
    assert_eq!(done, "[*]", "last step completes the checklist");
    assert!(
        matches!(
            run(&event("git push"), tmp.path()).unwrap(),
            HookResponse::Approve
        ),
        "complete checklist approves the retry"
    );

    let log = fs::read_to_string(dir.join("audit.log")).unwrap();
    let steps: Vec<serde_json::Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|v: &serde_json::Value| v["event"] == "step_done")
        .collect();
    assert_eq!(steps.len(), 2, "one audit line per recorded step: {log}");
    assert_eq!(steps.first().unwrap()["evidence"], "read diff", "{log}");
}

#[test]
fn rejects_blank_evidence_wrong_step_and_bad_outcome() {
    let tmp = TempDir::new().unwrap();
    let dir = blocked(tmp.path());

    for (step, next, evidence) in [
        ("review", Some("ship"), "  "),
        ("ship", None, "skipping ahead"),
        ("review", None, "no outcome given"),
        ("review", Some("nowhere"), "bad outcome"),
    ] {
        let err = record_step_done(&dir, "s1", "gate", step, next, evidence);
        assert!(err.is_err(), "{step} {next:?} {evidence:?} must fail");
    }
    assert_eq!(current(&dir), "review", "failed calls must not advance");
    assert!(
        record_step_done(&dir, "other", "gate", "review", None, "x").is_err(),
        "unknown session must fail"
    );
}

#[test]
fn completed_checklist_is_a_no_op() {
    let tmp = TempDir::new().unwrap();
    let dir = blocked(tmp.path());
    record_step_done(&dir, "s1", "gate", "review", Some("abort"), "e").unwrap();
    record_step_done(&dir, "s1", "gate", "abort", None, "e").unwrap();
    let again = record_step_done(&dir, "s1", "gate", "abort", None, "e").unwrap();
    assert_eq!(
        again, "[*]",
        "repeat call on a complete checklist changes nothing"
    );
}

#[test]
fn only_a_plain_step_command_skips_the_gates() {
    assert!(
        is_step_command(&event(
            "steplock step done gate review --session s1 --evidence \"ran git push --dry-run\""
        )),
        "plain record command is never gated"
    );
    for cmd in [
        "steplock step done gate review --evidence x && git push",
        "steplock step done gate review --evidence x; git push",
        "steplock step done gate review --evidence $(git push)",
        "git push # steplock step done",
    ] {
        assert!(!is_step_command(&event(cmd)), "{cmd} must still be gated");
    }
}
