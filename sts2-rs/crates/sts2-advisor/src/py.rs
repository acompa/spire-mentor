//! Python's reading of a JSON value, for code ported from the Python advisor.
//!
//! The ported functions took Python dicts and leaned on Python semantics:
//! `x or {}` (truthiness), `str(x)` (`None` -> `"None"`), `==` (`1 == 1.0 ==
//! True`, a tuple equal to a list of the same items once both are JSON). A
//! port that used JSON's own equality or `Value::to_string` would quietly
//! change which boards compare equal; these keep the old answers.

use std::cmp::Ordering;

use serde_json::{Map, Value};

/// The empty object `or {}` falls back to.
static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);

/// Python truthiness.
#[must_use]
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `d.get(key)`: `Null` when `d` is not an object or lacks `key`.
#[must_use]
pub fn get<'a>(d: &'a Value, key: &str) -> &'a Value {
    d.get(key).unwrap_or(&Value::Null)
}

/// `d.get(key) or {}` for a value used as a dict.
#[must_use]
pub fn obj<'a>(d: &'a Value, key: &str) -> &'a Map<String, Value> {
    as_obj(get(d, key))
}

/// `v or {}`.
#[must_use]
pub fn as_obj(v: &Value) -> &Map<String, Value> {
    match v {
        Value::Object(o) => o,
        _ => &EMPTY,
    }
}

/// `d.get(key) or []` for a value used as a list.
#[must_use]
pub fn list<'a>(d: &'a Value, key: &str) -> &'a [Value] {
    as_list(get(d, key))
}

/// `v or []`.
#[must_use]
pub fn as_list(v: &Value) -> &[Value] {
    match v {
        Value::Array(a) => a,
        _ => &[],
    }
}

/// `m.get(key)` on a map: `Null` when absent.
#[must_use]
pub fn mget<'a>(m: &'a Map<String, Value>, key: &str) -> &'a Value {
    m.get(key).unwrap_or(&Value::Null)
}

/// `str(v)`.
#[must_use]
pub fn str_of(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        // `str` of a list or dict is its `repr`.
        Value::Array(_) | Value::Object(_) => repr(v),
        other => other.to_string(),
    }
}

/// `repr(v)`, for the messages the mirror puts in a divergence. A Python
/// tuple arrived as an array and prints as a list; a dict prints in key
/// order rather than insertion order.
#[must_use]
pub fn repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => repr_str(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", repr_str(k), repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// `repr(s)` for a `str`: single quotes unless the text holds one and no
/// double quote.
fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `str(v or "")`.
#[must_use]
pub fn str_of_or_empty(v: &Value) -> String {
    if truthy(v) {
        str_of(v)
    } else {
        String::new()
    }
}

/// `f"{x:g}"`: six significant digits, trailing zeros (and a bare point)
/// dropped, exponent form outside `1e-4 <= |x| < 1e6`.
#[must_use]
pub fn fmt_g(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    #[allow(clippy::cast_possible_truncation)]
    let exp = x.abs().log10().floor() as i32;
    if !(-4..6).contains(&exp) {
        let s = format!("{x:.5e}");
        let (mantissa, e) = s.split_once('e').unwrap_or((&s, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{mantissa}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let decimals = usize::try_from(5 - exp).unwrap_or(0);
    let s = format!("{x:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// `str.title()`: each run of letters starts upper-case, the rest lower.
#[must_use]
pub fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for c in s.chars() {
        if prev_cased {
            out.extend(c.to_lowercase());
        } else {
            out.extend(c.to_uppercase());
        }
        prev_cased = c.is_alphabetic();
    }
    out
}

/// `int(v)` for a count. Python raises on `None` and on a non-numeric
/// string; no wire sends either where this is used, and 0 is the value the
/// key's own default (`p.get("amount", 0)`) gives.
#[must_use]
pub fn int_of(v: &Value) -> i64 {
    match v {
        Value::Bool(b) => i64::from(*b),
        Value::Number(n) => n
            .as_i64()
            // Truncation toward zero is what `int(2.7)` does.
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// A number's value under Python `==` (`True` is 1).
fn numeric(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Python `==` on two JSON-shaped values. Tuples and lists both arrive as
/// arrays, which is the comparison the Python code made: it only ever
/// compared a tuple with a tuple, or a list with a list.
#[must_use]
pub fn eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(lhs), Value::Number(rhs)) => match (lhs.as_i64(), rhs.as_i64()) {
            (Some(i), Some(j)) => i == j,
            _ => match (lhs.as_u64(), rhs.as_u64()) {
                (Some(i), Some(j)) => i == j,
                _ => lhs.as_f64() == rhs.as_f64(),
            },
        },
        (Value::Bool(_), Value::Number(_)) | (Value::Number(_), Value::Bool(_)) => {
            numeric(left) == numeric(right)
        }
        (Value::Array(lhs), Value::Array(rhs)) => {
            lhs.len() == rhs.len() && lhs.iter().zip(rhs).all(|(p, q)| eq(p, q))
        }
        (Value::Object(lhs), Value::Object(rhs)) => {
            lhs.len() == rhs.len()
                && lhs
                    .iter()
                    .all(|(k, v)| rhs.get(k).is_some_and(|w| eq(v, w)))
        }
        _ => left == right,
    }
}

/// Python's `<` order for the values a sort here meets (strings, numbers,
/// `None` last as a tie-breaker so a sort never panics where Python would
/// have raised).
#[must_use]
pub fn cmp(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.cmp(y),
        _ => match (numeric(a), numeric(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => rank(a)
                .cmp(&rank(b))
                .then_with(|| a.to_string().cmp(&b.to_string())),
        },
    }
}

fn rank(v: &Value) -> u8 {
    match v {
        Value::Bool(_) | Value::Number(_) => 0,
        Value::String(_) => 1,
        Value::Array(_) => 2,
        Value::Object(_) => 3,
        Value::Null => 4,
    }
}

/// Python's `round(x, n)`: the correctly rounded decimal, ties to even.
#[must_use]
pub fn round(x: f64, n: usize) -> f64 {
    format!("{x:.n$}").parse().unwrap_or(x)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn equality_is_pythons() {
        assert!(eq(&json!(1), &json!(1.0)));
        assert!(eq(&json!(true), &json!(1)));
        assert!(!eq(&json!(1), &json!("1")));
        assert!(eq(
            &json!([1, null, {"a": 2}]),
            &json!([1.0, null, {"a": 2}])
        ));
        assert!(!eq(&json!([1]), &json!([1, 2])));
    }

    #[test]
    fn str_and_truthiness_are_pythons() {
        assert_eq!(str_of(&Value::Null), "None");
        assert_eq!(str_of(&json!(true)), "True");
        assert_eq!(str_of(&json!("Nibbit")), "Nibbit");
        assert!(!truthy(&json!({})));
        assert!(!truthy(&json!(0)));
        assert!(truthy(&json!([0])));
    }

    #[test]
    fn repr_is_pythons() {
        assert_eq!(repr(&json!(null)), "None");
        assert_eq!(repr(&json!("Nibbit")), "'Nibbit'");
        assert_eq!(repr(&json!("it's")), "\"it's\"");
        assert_eq!(repr(&json!("a\nb")), "'a\\nb'");
        assert_eq!(repr(&json!([1, true, null])), "[1, True, None]");
        assert_eq!(repr(&json!({"a": 2.5})), "{'a': 2.5}");
    }

    #[test]
    fn int_truncates_like_python() {
        assert_eq!(int_of(&json!(2.7)), 2);
        assert_eq!(int_of(&json!(-2.7)), -2);
        assert_eq!(int_of(&json!(true)), 1);
        assert_eq!(int_of(&json!("3")), 3);
    }
}
