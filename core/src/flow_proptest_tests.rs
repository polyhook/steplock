//! Property-based tests for `flow`.
use proptest::prelude::*;

use super::*;

fn make_single_step_mmd(state: &str) -> String {
    format!("stateDiagram-v2\n    [*] --> {state}\n    {state} --> [*]\n    {state} : A step\n")
}

proptest! {
    /// A syntactically valid single-step diagram always parses successfully.
    #[test]
    fn valid_single_step_always_parses(state in "[a-z][a-z0-9_]{0,15}") {
        let mmd = make_single_step_mmd(&state);
        let g = parse_mmd("prop.mmd", &mmd).expect("valid diagram must parse");
        prop_assert_eq!(g.initial, vec![state.clone()]);
        prop_assert_eq!(g.order, vec![state]);
    }

    /// Every state in `initial` also appears in `order`.
    #[test]
    fn initial_states_in_order(state in "[a-z][a-z0-9_]{0,15}") {
        let mmd = make_single_step_mmd(&state);
        if let Ok(g) = parse_mmd("prop.mmd", &mmd) {
            for s in &g.initial {
                prop_assert!(g.order.contains(s));
            }
        }
    }

    /// `pending_after([])` equals `order` for any valid diagram.
    #[test]
    fn pending_after_empty_equals_order(state in "[a-z][a-z0-9_]{0,15}") {
        let mmd = make_single_step_mmd(&state);
        if let Ok(g) = parse_mmd("prop.mmd", &mmd) {
            prop_assert_eq!(g.pending_after(&[]), g.order);
        }
    }

    /// `pending_after(order)` is always empty.
    #[test]
    fn pending_after_all_is_empty(state in "[a-z][a-z0-9_]{0,15}") {
        let mmd = make_single_step_mmd(&state);
        if let Ok(g) = parse_mmd("prop.mmd", &mmd) {
            let all = g.order.clone();
            prop_assert!(g.pending_after(&all).is_empty());
        }
    }

    /// Arbitrary strings never panic — `parse_mmd` always returns `Ok` or `Err`.
    #[test]
    fn parse_never_panics(input in any::<String>()) {
        let _: crate::Result<_> = parse_mmd("fuzz.mmd", &input);
    }
}
