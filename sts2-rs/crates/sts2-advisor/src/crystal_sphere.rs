//! The Crystal Sphere: which cell to divine (GH #1426; was
//! `crystal_sphere_policy.choose` and `realgame_advisor._crystal_sphere_advice`).
//!
//! The policy is the digger (`sts2_content::events::crystal_sphere_digger`).
//! It needs what the screen shows: the fogged cells, the item class of every
//! cleared cell showing a piece (`revealed`) and the divinations left and at
//! the start. A message without all three, or anything the digger cannot
//! answer, falls back to the fixed block order (GH #872), and the note says
//! so.
//!
//! The fallback rule. The minigame hides fifteen items on an 11x11 grid and
//! reveals an item only once EVERY cell it covers is clear; each click is
//! the default Big tool (the cell and its eight neighbours). The relic is
//! 4x4, so only adjacent, non-overlapping 3x3 blocks can uncover it:
//! [`CENTERS`] clears a 3-wide column first (the 3-divination option), then
//! the column beside it (the 6-divination option). Past the list, the cell
//! clearing the most still-hidden cells wins, nearest the grid centre on a
//! tie. Twin of `spire-mentor-mod/CrystalSpherePick.cs`: keep the orders
//! identical.

use serde_json::{json, Value};

use crate::i18n::{is_zh, tr, trf};
use crate::py::{self, get, list, truthy};

/// `_defaultWidth` / `_defaultHeight` (`CrystalSphereMinigame.cs`).
pub const GRID_SIZE: i64 = 11;

/// Big-tool centres, in order: column x 2..4, then x 5..7.
pub const CENTERS: [(i64, i64); 6] = [(3, 2), (3, 5), (3, 8), (6, 2), (6, 5), (6, 8)];

/// The deciding policy, as the action's `annotations` name it.
pub const POLICY_NAME: &str = "crystal_sphere_rule";
pub const DIGGER_POLICY_NAME: &str = "crystal_sphere_digger";

/// A policy's name in a note: its id in English, its name in Chinese.
fn policy_label(policy: &'static str) -> &'static str {
    match policy {
        POLICY_NAME if is_zh() => "水晶球规则",
        DIGGER_POLICY_NAME if is_zh() => "水晶球挖掘器",
        other => other,
    }
}

/// `pick_cell`: the index into `hidden` of the cell the rule divines.
#[must_use]
pub fn pick_cell(hidden: &[(i64, i64)]) -> Option<usize> {
    if hidden.is_empty() {
        return None;
    }
    if let Some(i) = CENTERS
        .iter()
        .find_map(|c| hidden.iter().position(|h| h == c))
    {
        return Some(i);
    }
    let mid = GRID_SIZE / 2;
    let key = |i: usize| {
        let (x, y) = hidden[i];
        let mut cover = 0;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if hidden.contains(&(x + dx, y + dy)) {
                    cover += 1;
                }
            }
        }
        (-cover, (x - mid).pow(2) + (y - mid).pow(2), i)
    };
    (0..hidden.len()).min_by_key(|&i| key(i))
}

/// `int(v)`, or the Python exception's `"Type: message"` it raised.
fn py_int(v: Option<&Value>, key: &str) -> Result<i64, String> {
    match v {
        None => Err(format!("KeyError: {}", py::repr(&json!(key)))),
        Some(Value::Bool(b)) => Ok(i64::from(*b)),
        #[allow(clippy::cast_possible_truncation)]
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.is_finite())
                    .map(|f| f.trunc() as i64)
            })
            .ok_or_else(|| "ValueError: cannot convert float to integer".to_string()),
        Some(Value::String(s)) => s.trim().parse().map_err(|_| {
            format!(
                "ValueError: invalid literal for int() with base 10: {}",
                py::repr(&json!(s))
            )
        }),
        Some(other) => Err(format!(
            "TypeError: int() argument must be a string, a bytes-like object or a real \
             number, not '{}'",
            match other {
                Value::Null => "NoneType",
                Value::Array(_) => "list",
                _ => "dict",
            }
        )),
    }
}

/// `_digger_cell`: the digger's cell for this message, or the reason it
/// cannot answer (`"Type: message"`).
fn digger_cell(state: &Value, hidden: &[(i64, i64)]) -> Result<(i64, i64), String> {
    use sts2_content::events::crystal_sphere_digger::{pick, ItemClass, SphereView};
    let cell_bit = |x: i64, y: i64| -> Result<u32, String> {
        if (0..GRID_SIZE).contains(&x) && (0..GRID_SIZE).contains(&y) {
            Ok(u32::try_from(x * GRID_SIZE + y).unwrap_or(0))
        } else {
            Err(format!(
                "ValueError: crystal sphere cell ({x},{y}) is outside the 11x11 grid"
            ))
        }
    };
    let mut shown = Vec::new();
    for r in list(state, "revealed") {
        let x = py_int(r.get("x"), "x")?;
        let y = py_int(r.get("y"), "y")?;
        let item = py::str_of(r.get("item").ok_or("KeyError: 'item'")?);
        shown.push((x, y, item));
    }
    // Both `int(...)`s run before the binding converts either to `u32`.
    let left = py_int(state.get("divinations_left"), "divinations_left")?;
    let total = py_int(state.get("divinations_total"), "divinations_total")?;
    // The binding's argument conversions, in its argument order: the
    // coordinates to `i32`, then the counts to `u32` (PyO3's wording).
    let fits = |ok: bool, n: i64| {
        if ok {
            Ok(())
        } else if n < 0 {
            Err("OverflowError: number too small to fit in target type".to_string())
        } else {
            Err("OverflowError: number too large to fit in target type".to_string())
        }
    };
    let coords = hidden
        .iter()
        .copied()
        .chain(shown.iter().map(|&(x, y, _)| (x, y)));
    for (x, y) in coords {
        for v in [x, y] {
            fits(i32::try_from(v).is_ok(), v)?;
        }
    }
    let unsigned =
        |n: i64| fits(u32::try_from(n).is_ok(), n).map(|()| u32::try_from(n).unwrap_or(0));
    let remaining = unsigned(left)?;
    let total = unsigned(total)?;
    let mut fog = 0u128;
    for &(x, y) in hidden {
        fog |= 1u128 << cell_bit(x, y)?;
    }
    let mut pieces = Vec::with_capacity(shown.len());
    for (x, y, item) in shown {
        cell_bit(x, y)?;
        let class = match item.as_str() {
            "relic" => ItemClass::Relic,
            "potion" => ItemClass::Potion,
            "card" => ItemClass::Card,
            "curse" => ItemClass::Curse,
            "gold" => ItemClass::Gold,
            other => {
                return Err(format!(
                    "ValueError: crystal sphere item {other:?} is not relic/potion/card/curse/gold"
                ))
            }
        };
        #[allow(clippy::cast_possible_truncation)]
        pieces.push(((x as i32, y as i32), class));
    }
    let cell = pick(&SphereView {
        hidden: fog,
        shown: pieces,
        remaining,
        total,
    })
    .ok_or("ValueError: no pick (no fogged cell or no divination left)")?;
    Ok((i64::from(cell.0), i64::from(cell.1)))
}

/// An option's `index` when it is an int (`True` included, as Python's
/// `isinstance(_, int)` has it), else its position.
fn option_index(pos: usize, o: &Value) -> Value {
    match o.get("index") {
        Some(v @ Value::Number(n)) if n.is_i64() || n.is_u64() => v.clone(),
        Some(v @ Value::Bool(_)) => v.clone(),
        _ => json!(pos),
    }
}

/// `choose`'s answer: the action (`None` when no enabled option is usable),
/// and the warning it logs when the digger's fields were there but the rule
/// answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub action: Option<Value>,
    pub warning: Option<String>,
}

/// `choose`: the bridge answer to a `crystal_sphere` message. Proceed wins when offered (a cell click
/// after the divinations are spent loops the screen); then the digger, when
/// the message carries its three fields; then [`pick_cell`].
#[must_use]
pub fn choose(state: &Value) -> Choice {
    choose_with(state, digger_cell)
}

/// [`choose`] with the digger standing in (`digger(state, hidden)`).
fn choose_with(
    state: &Value,
    digger: impl Fn(&Value, &[(i64, i64)]) -> Result<(i64, i64), String>,
) -> Choice {
    let enabled: Vec<(usize, &Value)> = list(state, "options")
        .iter()
        .enumerate()
        .filter(|(_, o)| o.is_object() && o.get("enabled").is_none_or(truthy))
        .collect();
    let index = |pos: usize, o: &Value| option_index(pos, o);
    let answer = |pos: usize, o: &Value, note: String, policy: &str| {
        json!({
            "action": "choose",
            "index": index(pos, o),
            "annotations": {"note": note, "policy": policy},
        })
    };
    if let Some((pos, o)) = enabled
        .iter()
        .find(|(_, o)| get(o, "action").as_str() == Some("proceed"))
    {
        let note = trf(
            "{policy}: divinations spent -> proceed",
            &[("policy", &policy_label(POLICY_NAME))],
        );
        return Choice {
            action: Some(answer(*pos, o, note, POLICY_NAME)),
            warning: None,
        };
    }
    let mut cells: Vec<(usize, &Value, (i64, i64))> = Vec::new();
    for &(pos, o) in &enabled {
        if get(o, "action").as_str() != Some("divine_cell") {
            continue;
        }
        if let (Ok(x), Ok(y)) = (py_int(o.get("x"), "x"), py_int(o.get("y"), "y")) {
            cells.push((pos, o, (x, y)));
        }
    }
    let hidden: Vec<(i64, i64)> = cells.iter().map(|c| c.2).collect();
    let mut fell_back = String::new();
    let mut warning = None;
    if ["divinations_left", "divinations_total", "revealed"]
        .iter()
        .all(|k| !get(state, k).is_null())
    {
        match digger(state, &hidden) {
            Err(why) => fell_back = trf(" (digger fell back: {why})", &[("why", &why)]),
            Ok(cell) => {
                if let Some(i) = hidden.iter().position(|h| *h == cell) {
                    let (pos, o, (x, y)) = cells[i];
                    let note = trf(
                        "{policy}: divine cell ({x},{y}) ({left}/{total} divinations left)",
                        &[
                            ("policy", &policy_label(DIGGER_POLICY_NAME)),
                            ("x", &x),
                            ("y", &y),
                            ("left", &py::str_of(get(state, "divinations_left"))),
                            ("total", &py::str_of(get(state, "divinations_total"))),
                        ],
                    );
                    return Choice {
                        action: Some(answer(pos, o, note, DIGGER_POLICY_NAME)),
                        warning: None,
                    };
                }
                fell_back = trf(
                    " (digger fell back: its cell ({x}, {y}) is not offered)",
                    &[("x", &cell.0), ("y", &cell.1)],
                );
            }
        }
        // A rule pick must never pass for the digger's.
        warning = Some(format!(
            "crystal sphere: the digger did not answer; playing the #872 rule{fell_back}"
        ));
    }
    let action = pick_cell(&hidden).map(|i| {
        let (pos, o, (x, y)) = cells[i];
        let note = trf(
            "{policy}: divine cell ({x},{y}) (fixed 3x3 block order){fell_back}",
            &[
                ("policy", &policy_label(POLICY_NAME)),
                ("x", &x),
                ("y", &y),
                ("fell_back", &fell_back),
            ],
        );
        answer(pos, o, note, POLICY_NAME)
    });
    Choice { action, warning }
}

/// `_crystal_sphere_advice`: the wire advice for the Crystal Sphere (no
/// run-policy net), and the warning [`choose`] logged.
#[must_use]
pub fn advice(state: &Value) -> (Value, Option<String>) {
    let Choice { action, warning } = choose(state);
    let Some(action) = action else {
        return (
            json!({
                "note": tr("crystal sphere: no enabled cell or proceed option"),
                "hand_back": tr("the crystal sphere has nothing to choose"),
            }),
            warning,
        );
    };
    let index = get(&action, "index").clone();
    // `choose` names an option by its `index`, or its position without one.
    let chosen = list(state, "options")
        .iter()
        .enumerate()
        .find(|(pos, o)| o.is_object() && py::eq(&option_index(*pos, o), &index))
        .map(|(_, o)| o);
    let label = match chosen {
        Some(o) if get(o, "action").as_str() == Some("divine_cell") => trf(
            "reveal cell ({x},{y})",
            &[
                ("x", &py::str_of(get(o, "x"))),
                ("y", &py::str_of(get(o, "y"))),
            ],
        ),
        _ => tr("proceed").to_string(),
    };
    let annotations = get(&action, "annotations");
    (
        json!({
            "best": {"action": "choose", "index": index, "label": label},
            "note": annotations["note"],
            "policy": annotations["policy"],
        }),
        warning,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_clears_the_columns_in_order_then_the_widest_cell() {
        assert_eq!(pick_cell(&[(0, 0), (3, 5), (3, 2)]), Some(2));
        // (0,0) and (1,1) clear all four; (1,1) is nearer the centre.
        assert_eq!(pick_cell(&[(0, 0), (1, 1), (1, 0), (0, 1)]), Some(1));
        assert_eq!(pick_cell(&[]), None);
    }

    #[test]
    fn proceed_wins_and_a_bad_digger_message_falls_back_with_a_reason() {
        let proceed = json!({"options": [
            {"index": 0, "action": "divine_cell", "x": 3, "y": 2},
            {"index": 1, "action": "proceed"},
        ]});
        assert_eq!(choose(&proceed).action.unwrap()["index"], 1);
        let bad = json!({
            "options": [{"index": 4, "action": "divine_cell", "x": 3, "y": 2}],
            "divinations_left": 3, "divinations_total": 3,
            "revealed": [{"x": 1, "y": 1, "item": "statue"}],
        });
        let c = choose(&bad);
        assert_eq!(c.action.unwrap()["index"], 4);
        assert!(c
            .warning
            .unwrap()
            .contains("is not relic/potion/card/curse/gold"));
    }

    #[test]
    fn a_digger_cell_that_is_not_offered_falls_back_to_the_rule() {
        let state = json!({
            "options": [
                {"index": 0, "action": "divine_cell", "x": 0, "y": 0},
                {"index": 1, "action": "divine_cell", "x": 3, "y": 2},
            ],
            "divinations_left": 6, "divinations_total": 6, "revealed": [],
        });
        let c = choose_with(&state, |_, _| Ok((9, 9)));
        let action = c.action.unwrap();
        assert_eq!(action["index"], 1);
        assert_eq!(action["annotations"]["policy"], POLICY_NAME);
        assert!(action["annotations"]["note"]
            .as_str()
            .unwrap()
            .ends_with("(digger fell back: its cell (9, 9) is not offered)"));
        assert!(c.warning.is_some());
    }
}
