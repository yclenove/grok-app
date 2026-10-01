//! Short, neutral-to-neutral sequences. Preflight must finish before any input.
//! Not a timed-drag executor and never an application-effect acknowledgment.
use crate::InputAction;
use grok_computer_use_core::execution::ActionCancellation;
use std::collections::BTreeSet;

pub(crate) fn validate_balanced(actions: &[InputAction]) -> Result<(), String> {
    if !(2..=8).contains(&actions.len()) {
        return Err("compound EI input requires 2..=8 events".into());
    }
    let mut keys = BTreeSet::new();
    let mut buttons = BTreeSet::new();
    let mut scrolling = false;
    for action in actions {
        action.validate()?;
        match *action {
            InputAction::Key { code, pressed } | InputAction::Button { code, pressed } => {
                let held = if matches!(action, InputAction::Key { .. }) {
                    &mut keys
                } else {
                    &mut buttons
                };
                if if pressed {
                    !held.insert(code)
                } else {
                    !held.remove(&code)
                } {
                    return Err("compound EI input has an unowned state transition".into());
                }
            }
            InputAction::Scroll { dx, dy } => scrolling |= dx != 0.0 || dy != 0.0,
            InputAction::ScrollDiscrete { dx, dy } => scrolling |= dx != 0 || dy != 0,
            InputAction::CancelScroll => scrolling = false,
            InputAction::ReleaseAll => {
                return Err("compound input cannot release unrelated owned state".into());
            }
            _ => {}
        }
    }
    if !keys.is_empty() || !buttons.is_empty() || scrolling {
        return Err("compound EI input must end with no held input".into());
    }
    Ok(())
}

pub(crate) fn execute(
    actions: Vec<InputAction>,
    cancellation: &ActionCancellation,
    mut send: impl FnMut(InputAction) -> Result<(), String>,
) -> Result<(), String> {
    let mut attempted = false;
    for action in actions {
        let result = cancellation.check().and_then(|()| {
            attempted = true;
            send(action)
        });
        if let Err(error) = result {
            // Cleanup bypasses cancellation, but the native owner releases only
            // its synthetic state. Preflight required a neutral initial state.
            if attempted {
                send(InputAction::ReleaseAll)
                    .map_err(|cleanup| format!("{error}; EI cleanup uncertain: {cleanup}"))?;
            }
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tap() -> Vec<InputAction> {
        vec![
            InputAction::Key {
                code: 30,
                pressed: true,
            },
            InputAction::Key {
                code: 30,
                pressed: false,
            },
        ]
    }
    #[test]
    fn whole_plan_rejects_unbalanced_duplicate_invalid_and_cleanup_events() {
        assert!(validate_balanced(&tap()).is_ok());
        for actions in [
            vec![],
            vec![tap()[0].clone()],
            vec![tap()[0].clone(), tap()[0].clone()],
            vec![tap()[1].clone(), tap()[0].clone()],
            vec![
                InputAction::Relative {
                    dx: f64::NAN,
                    dy: 0.0,
                },
                tap()[1].clone(),
            ],
            vec![
                InputAction::Scroll { dx: 0.0, dy: 12.0 },
                InputAction::Relative { dx: 1.0, dy: 0.0 },
            ],
            vec![InputAction::ReleaseAll, InputAction::ReleaseAll],
            vec![InputAction::Relative { dx: 1.0, dy: 0.0 }; 9],
        ] {
            assert!(validate_balanced(&actions).is_err(), "{actions:?}");
        }
    }
    #[test]
    fn cancellation_between_press_and_release_cleans_up_without_replay() {
        let cancel = ActionCancellation::default();
        let mut events = Vec::new();
        let result = execute(tap(), &cancel, |event| {
            events.push(event);
            cancel.cancel();
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], InputAction::Key { pressed: true, .. }));
        assert!(matches!(events[1], InputAction::ReleaseAll));
        events.clear();
        assert!(execute(tap(), &cancel, |e| {
            events.push(e);
            Ok(())
        })
        .is_err());
        assert!(events.is_empty());
    }
    #[test]
    fn cancellation_after_scroll_sends_owned_cleanup_and_stops_motion() {
        let actions = vec![
            InputAction::Scroll { dx: 0.0, dy: 120.0 },
            InputAction::CancelScroll,
        ];
        assert!(validate_balanced(&actions).is_ok());
        let cancellation = ActionCancellation::default();
        let mut events = Vec::new();
        assert!(execute(actions, &cancellation, |event| {
            events.push(event);
            cancellation.cancel();
            Ok(())
        })
        .is_err());
        assert_eq!(events.len(), 2);
        assert!(matches!(events[1], InputAction::ReleaseAll));
    }
    #[test]
    fn uncertain_native_error_neutralizes_and_preserves_failure() {
        let mut events = Vec::new();
        let result = execute(tap(), &ActionCancellation::default(), |event| {
            events.push(event);
            if events.len() == 2 {
                Err("native failure".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.unwrap_err(), "native failure");
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2], InputAction::ReleaseAll));
    }
}
