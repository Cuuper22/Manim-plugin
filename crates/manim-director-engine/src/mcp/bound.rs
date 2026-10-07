//! Keeping tool output within the MCP byte budgets (OPS §1.7): lists are
//! halved and marked `truncated: true`, long strings are cut.

use serde_json::Value;

/// Shrinks `value` until it serializes within `max` bytes: the largest list
/// with more than one item is halved and its enclosing object gains
/// `"truncated": true`; otherwise the largest string is cut in half.
pub(super) fn bound(value: &mut Value, max: usize) {
    while size(value) > max {
        let Some(target) = largest(value, &mut Vec::new()) else {
            return;
        };
        if !shrink(value, &target.path) {
            return;
        }
    }
}

pub(super) fn size(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |encoded| encoded.len())
}

#[derive(Clone)]
enum Step {
    Key(String),
    Index(usize),
}

struct Target {
    path: Vec<Step>,
    size: usize,
}

/// The biggest shrinkable list or string under `value`.
fn largest(value: &Value, path: &mut Vec<Step>) -> Option<Target> {
    let mut best = match value {
        Value::Array(items) if items.len() > 1 => Some(Target {
            path: path.clone(),
            size: size(value),
        }),
        Value::String(text) if text.len() > 64 => Some(Target {
            path: path.clone(),
            size: text.len(),
        }),
        _ => None,
    };
    let children: Box<dyn Iterator<Item = (Step, &Value)>> = match value {
        Value::Array(items) => Box::new(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| (Step::Index(index), item)),
        ),
        Value::Object(fields) => Box::new(
            fields
                .iter()
                .map(|(key, item)| (Step::Key(key.clone()), item)),
        ),
        _ => Box::new(std::iter::empty()),
    };
    for (step, child) in children {
        path.push(step);
        if let Some(found) = largest(child, path) {
            if best.as_ref().is_none_or(|best| found.size > best.size) {
                best = Some(found);
            }
        }
        path.pop();
    }
    best
}

fn shrink(root: &mut Value, path: &[Step]) -> bool {
    let tail_key =
        matches!(path.last(), Some(Step::Key(key)) if key.ends_with("tail") || key == "traceback");
    let Some(target) = resolve(root, path) else {
        return false;
    };
    match target {
        Value::Array(items) => {
            items.truncate(items.len() / 2);
            mark_truncated(root, path);
            true
        }
        Value::String(text) => {
            *text = halve(text, tail_key);
            true
        }
        _ => false,
    }
}

fn resolve<'a>(value: &'a mut Value, path: &[Step]) -> Option<&'a mut Value> {
    path.iter().try_fold(value, |value, step| match step {
        Step::Key(key) => value.get_mut(key.as_str()),
        Step::Index(index) => value.get_mut(*index),
    })
}

/// Flags the nearest object holding the shortened list.
fn mark_truncated(root: &mut Value, path: &[Step]) {
    for end in (0..path.len()).rev() {
        if let Some(Value::Object(fields)) = resolve(root, &path[..end]) {
            fields.insert("truncated".into(), Value::Bool(true));
            return;
        }
    }
    if let Value::Object(fields) = root {
        fields.insert("truncated".into(), Value::Bool(true));
    }
}

/// Half of `text` with an ellipsis; tails keep their end.
fn halve(text: &str, keep_end: bool) -> String {
    let keep = text.len() / 2;
    if keep_end {
        let mut start = text.len() - keep;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        format!("…{}", &text[start..])
    } else {
        let mut end = keep;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn long_lists_are_halved_and_marked() {
        let mut value = json!({
            "result": {"findings": (0..400).map(|index| json!({"message": format!("finding {index}")})).collect::<Vec<_>>()},
            "error": null,
        });
        bound(&mut value, 2 * 1024);
        assert!(size(&value) <= 2 * 1024);
        assert_eq!(value["result"]["truncated"], true);
        let kept = value["result"]["findings"].as_array().unwrap();
        assert!(!kept.is_empty());
        assert_eq!(kept[0]["message"], "finding 0");
    }

    #[test]
    fn long_strings_are_cut_and_tails_keep_their_end() {
        let tail = format!("{}THE END", "x".repeat(70_000));
        let mut value =
            json!({"error": {"message": "y".repeat(70_000), "data": {"stderr_tail": tail}}});
        bound(&mut value, 48 * 1024);
        assert!(size(&value) <= 48 * 1024);
        assert!(value["error"]["data"]["stderr_tail"]
            .as_str()
            .unwrap()
            .ends_with("THE END"));
        assert!(value["error"]["message"].as_str().unwrap().starts_with('y'));
    }
}
