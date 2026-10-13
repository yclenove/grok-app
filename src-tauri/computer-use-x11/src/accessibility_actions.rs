//! Action IDs must come from GetName, not the localized GetActions tuple.
use super::{err, proxy, Connection, Instant, Object, ACTION};

const MAX_ACTIONS: i32 = 64;
const MAX_NAME_BYTES: usize = 128;

pub(super) fn click_index(
    connection: &Connection,
    object: &Object,
    deadline: Instant,
) -> Result<Option<i32>, String> {
    check_deadline(deadline)?;
    let action = proxy(connection, object, ACTION)?;
    let count: i32 = action.get_property("NActions").map_err(err)?;
    // Old ATK bridges (including Ubuntu 22.04's 2.38) write four strings into
    // GetActions' three-string struct and abort the application. New bridges
    // return localized names there, which cannot identify click/activate.
    let names = read_names(count, deadline, |index| {
        action.call("GetName", &(index,)).map_err(err)
    })?;
    check_deadline(deadline)?;
    let current_count: i32 = action.get_property("NActions").map_err(err)?;
    check_deadline(deadline)?;
    if current_count != count {
        return Err("AT-SPI action count changed during discovery".into());
    }
    Ok(primary_action_index(&names))
}

fn check_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        return Err("AT-SPI action discovery deadline".into());
    }
    Ok(())
}

fn read_names(
    count: i32,
    deadline: Instant,
    mut get_name: impl FnMut(i32) -> Result<String, String>,
) -> Result<Vec<String>, String> {
    if !(0..=MAX_ACTIONS).contains(&count) {
        return Err("AT-SPI action count exceeds discovery bounds".into());
    }
    let mut names = Vec::with_capacity(count as usize);
    for index in 0..count {
        check_deadline(deadline)?;
        let name = get_name(index)?;
        if name.len() > MAX_NAME_BYTES {
            return Err("AT-SPI action name exceeds discovery bounds".into());
        }
        names.push(name);
    }
    Ok(names)
}

fn primary_action_index(names: &[String]) -> Option<i32> {
    // GTK can expose click, press and release together. Only a complete primary
    // action is eligible; never treat a bare press as a completed click.
    for name in ["click", "activate"] {
        let mut matches = names
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.eq_ignore_ascii_case(name));
        if let Some((index, _)) = matches.next() {
            return matches.next().is_none().then_some(index as i32);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn gtk_press_release_are_not_ambiguous_complete_clicks() {
        let names = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            primary_action_index(&names(&["click", "press", "release"])),
            Some(0)
        );
        assert_eq!(primary_action_index(&names(&["Click"])), Some(0));
        assert_eq!(
            primary_action_index(&names(&["press", "activate", "release"])),
            Some(1)
        );
        assert_eq!(primary_action_index(&names(&["press", "release"])), None);
        assert_eq!(primary_action_index(&names(&["click", "click"])), None);
        assert_eq!(
            primary_action_index(&names(&["activate", "activate"])),
            None
        );
        assert_eq!(primary_action_index(&names(&["点击", "Activer"])), None);
    }

    #[test]
    fn malformed_counts_and_expired_discovery_never_query_names() {
        let deadline = Instant::now() + Duration::from_secs(1);
        for count in [-1, MAX_ACTIONS + 1, i32::MAX] {
            assert!(read_names(count, deadline, |_| panic!(
                "invalid count queried provider"
            ))
            .is_err());
        }
        assert!(read_names(1, Instant::now(), |_| panic!(
            "expired query reached provider"
        ))
        .is_err());
        assert!(
            read_names(0, deadline, |_| panic!("zero actions queried provider"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn partial_or_oversized_discovery_cannot_publish_a_click() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut queried = Vec::new();
        let result = read_names(3, deadline, |index| {
            queried.push(index);
            match index {
                0 => Ok("click".into()),
                _ => Err("provider disconnected".into()),
            }
        });
        assert!(result.is_err());
        assert_eq!(queried, [0, 1]);
        assert!(read_names(1, deadline, |_| Ok("x".repeat(MAX_NAME_BYTES + 1))).is_err());
    }

    #[test]
    fn discovery_preserves_native_indices_at_the_count_boundary() {
        let names = read_names(
            MAX_ACTIONS,
            Instant::now() + Duration::from_secs(1),
            |index| {
                Ok(if index == MAX_ACTIONS - 1 {
                    "activate".into()
                } else {
                    "x".repeat(MAX_NAME_BYTES)
                })
            },
        )
        .unwrap();
        assert_eq!(primary_action_index(&names), Some(MAX_ACTIONS - 1));
    }
}
