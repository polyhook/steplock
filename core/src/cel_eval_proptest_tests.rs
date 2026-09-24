//! Property-based tests for `cel_eval`.
use std::collections::HashMap;

use proptest::prelude::*;

use super::*;
use crate::state::HookEvent;

fn arb_event() -> impl Strategy<Value = HookEvent> {
    (
        any::<String>(),
        any::<String>(),
        any::<String>(),
        any::<String>(),
    )
        .prop_map(|(event, tool, session_id, caller)| HookEvent {
            event,
            tool,
            session_id,
            caller,
            input: HashMap::new(),
            output: HashMap::new(),
        })
}

proptest! {
    /// `matches_event` never panics for arbitrary events and expressions.
    #[test]
    fn never_panics(event in arb_event(), expr in any::<Option<String>>()) {
        let _: crate::Result<bool> = matches_event(&event, &expr);
    }

    /// `None` expression always approves regardless of event content.
    #[test]
    fn none_always_approves(event in arb_event()) {
        prop_assert!(matches_event(&event, &None).unwrap());
    }

    /// Whitespace-only expression always approves.
    #[test]
    fn whitespace_always_approves(event in arb_event(), ws in " *") {
        prop_assert!(matches_event(&event, &Some(ws)).unwrap());
    }

    /// CEL literal `true` matches any event.
    #[test]
    fn cel_true_always_matches(event in arb_event()) {
        prop_assert!(matches_event(&event, &Some("true".to_owned())).unwrap());
    }

    /// CEL literal `false` never matches any event.
    #[test]
    fn cel_false_never_matches(event in arb_event()) {
        prop_assert!(!matches_event(&event, &Some("false".to_owned())).unwrap());
    }
}
