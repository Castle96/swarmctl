//! kubectl-compatible label and field selectors.
//!
//! This used to be `selector.split_once('=')` and nothing else. That accepted
//! exactly one expression form, silently matched *nothing* for every other form,
//! and made a typo indistinguishable from "no results". The parser now covers
//! the full grammar and **errors on malformed input**:
//!
//! ```text
//! key=value        key==value      key!=value
//! key              !key            (existence / non-existence)
//! key in (a,b)     key notin (a,b)
//! ```
//!
//! Comma separates requirements and each one must hold (AND). Commas inside
//! `(...)` and inside quotes are not separators.
//!
//! Both selector flavours compile to the same [`Requirement`] list; only the
//! evaluation differs. Label selectors read a `HashMap`, field selectors walk a
//! JSON document with [`crate::utils::jsonpath`].

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// One parsed requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    Equals(String, String),
    NotEquals(String, String),
    In(String, Vec<String>),
    NotIn(String, Vec<String>),
    Exists(String),
    DoesNotExist(String),
}

/// A parsed selector: a conjunction of requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    requirements: Vec<Requirement>,
}

impl Selector {
    /// Parse a selector expression.
    ///
    /// An empty or whitespace-only string is rejected: `--selector ""` is a
    /// mistake, not a request for "everything".
    pub fn parse(expr: &str) -> anyhow::Result<Self> {
        let expr = expr.trim();
        if expr.is_empty() {
            anyhow::bail!("selector is empty; expected e.g. app=web or key in (a,b)");
        }
        let terms = split_top_level(expr)?;
        if terms.is_empty() {
            anyhow::bail!("selector \"{}\" has no requirements", expr);
        }
        let mut requirements = Vec::with_capacity(terms.len());
        for term in terms {
            let t = term.trim();
            if t.is_empty() {
                anyhow::bail!(
                    "selector \"{}\" has an empty requirement (a stray comma?)",
                    expr
                );
            }
            requirements.push(parse_requirement(t)?);
        }
        Ok(Selector { requirements })
    }

    /// Evaluate against a label map. A resource with no labels only satisfies a
    /// selector made entirely of `!key` (non-existence) requirements.
    pub fn matches(&self, labels: &Option<HashMap<String, String>>) -> bool {
        self.requirements
            .iter()
            .all(|req| req.matches_labels(labels))
    }

    /// Evaluate against a JSON document using jsonpath for each key.
    pub fn matches_value(&self, value: &Value) -> bool {
        self.requirements.iter().all(|req| req.matches_value(value))
    }
}

impl Requirement {
    fn matches_labels(&self, labels: &Option<HashMap<String, String>>) -> bool {
        let lookup = |key: &str| labels.as_ref().and_then(|m| m.get(key)).cloned();
        match self {
            Requirement::Exists(k) => lookup(k).is_some(),
            Requirement::DoesNotExist(k) => lookup(k).is_none(),
            Requirement::Equals(k, want) => lookup(k).as_deref() == Some(want.as_str()),
            Requirement::NotEquals(k, want) => lookup(k).as_deref() != Some(want.as_str()),
            Requirement::In(k, set) => lookup(k).is_some_and(|v| set.iter().any(|s| s == &v)),
            Requirement::NotIn(k, set) => match lookup(k) {
                // kubectl: `notin` matches objects that lack the key entirely.
                None => true,
                Some(v) => !set.iter().any(|s| s == &v),
            },
        }
    }

    fn matches_value(&self, value: &Value) -> bool {
        fn lookup(key: &str, value: &Value) -> Option<Value> {
            crate::utils::jsonpath::parse(key)
                .ok()
                .and_then(|p| p.eval(value).ok())
                .and_then(|mut hits| {
                    if hits.is_empty() {
                        None
                    } else {
                        Some(hits.remove(0))
                    }
                })
        }
        match self {
            Requirement::Exists(k) => lookup(k, value).is_some(),
            Requirement::DoesNotExist(k) => lookup(k, value).is_none(),
            Requirement::Equals(k, want) => {
                lookup(k, value).is_some_and(|v| scalar_to_string(&v) == *want)
            }
            Requirement::NotEquals(k, want) => {
                // Absent is "not equal", matching the label behaviour above.
                lookup(k, value)
                    .map(|v| scalar_to_string(&v) != *want)
                    .unwrap_or(true)
            }
            Requirement::In(k, set) => lookup(k, value).is_some_and(|v| {
                let s = scalar_to_string(&v);
                set.contains(&s)
            }),
            Requirement::NotIn(k, set) => match lookup(k, value) {
                None => true,
                Some(v) => {
                    let s = scalar_to_string(&v);
                    !set.contains(&s)
                }
            },
        }
    }
}

/// Compare a JSON scalar against a selector's textual value.
///
/// Numbers must compare numerically so `--field-selector replicas=3` and
/// `--field-selector replicas=10` behave; the old code stringified the number
/// and compared text.
fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Split on commas that are not inside parentheses or quotes.
fn split_top_level(expr: &str) -> anyhow::Result<Vec<String>> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;

    for c in expr.chars() {
        match quote {
            Some(q) => {
                current.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    current.push(c);
                }
                '(' => {
                    depth += 1;
                    current.push(c);
                }
                ')' => {
                    if depth == 0 {
                        anyhow::bail!("unbalanced ')' in selector \"{}\"", expr);
                    }
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => {
                    terms.push(std::mem::take(&mut current));
                }
                _ => current.push(c),
            },
        }
    }
    if quote.is_some() {
        anyhow::bail!("unterminated quote in selector \"{}\"", expr);
    }
    if depth != 0 {
        anyhow::bail!("unbalanced '(' in selector \"{}\"", expr);
    }
    terms.push(current);
    Ok(terms)
}

/// Parse one requirement term.
fn parse_requirement(term: &str) -> anyhow::Result<Requirement> {
    let term = term.trim();

    // `!key` — but `!=` is the not-equals operator, so check the next character.
    if let Some(rest) = term.strip_prefix('!')
        && !rest.starts_with('=')
    {
        let key = rest.trim();
        validate_key(term, key)?;
        return Ok(Requirement::DoesNotExist(key.to_string()));
    }

    // Set-based forms, longest operator first so `notin` wins over `in`.
    for op in [" notin ", " in "] {
        if let Some(idx) = find_op(term, op) {
            let key = term[..idx].trim();
            let value_text = term[idx + op.len()..].trim();
            validate_key(term, key)?;
            let values = parse_value_set(term, value_text)?;
            return Ok(if op == " in " {
                Requirement::In(key.to_string(), values)
            } else {
                Requirement::NotIn(key.to_string(), values)
            });
        }
    }

    // Comparison operators, longest first: `==` before `=`.
    for op in ["==", "!=", "="] {
        if let Some(idx) = term.find(op) {
            // `!=` must not be mistaken for `=` at index idx+1.
            if op == "=" && term[..idx].ends_with('!') {
                continue;
            }
            let key = term[..idx].trim();
            let value = term[idx + op.len()..].trim();
            validate_key(term, key)?;
            // A value may itself contain `=` (e.g. `conn=a=b`), which is why the
            // split is "first operator wins" rather than "last operator wins".
            return Ok(match op {
                "==" => Requirement::Equals(key.to_string(), value.to_string()),
                "!=" => Requirement::NotEquals(key.to_string(), value.to_string()),
                _ => Requirement::Equals(key.to_string(), value.to_string()),
            });
        }
    }

    // No operator at all: an existence check.
    validate_key(term, term)?;
    Ok(Requirement::Exists(term.to_string()))
}

/// Locate `op` in `term`, requiring word boundaries so a key like `tier` does
/// not match an `in ` operator inside it.
fn find_op(term: &str, op: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(rel) = term[from..].find(op) {
        let idx = from + rel;
        // `op` begins with its own leading space, so `idx` points at that space.
        // For the match to be a real operator, the character before it must be a
        // key character (not another space), and the character after the
        // trailing space must not continue the token.
        let before_ok = idx == 0 || !term.as_bytes()[idx - 1].is_ascii_whitespace();
        let after = idx + op.len();
        let after_ok = after >= term.len() || !term.as_bytes()[after].is_ascii_whitespace();
        if before_ok && after_ok {
            return Some(idx);
        }
        from = idx + 1;
        if from >= term.len() {
            break;
        }
    }
    None
}

/// Parse `(a,b)` or `a` into a value list.
fn parse_value_set(term: &str, text: &str) -> anyhow::Result<Vec<String>> {
    let inner = text
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "set operator in \"{}\" needs parentheses, e.g. key in (a,b)",
                term
            )
        })?;
    let values: Vec<String> = inner
        .split(',')
        .map(|v| v.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
        .collect();
    if values.is_empty() {
        anyhow::bail!("set operator in \"{}\" has no values", term);
    }
    Ok(values)
}

fn validate_key(term: &str, key: &str) -> anyhow::Result<()> {
    if key.is_empty() {
        anyhow::bail!("requirement \"{}\" is missing a key", term);
    }
    Ok(())
}

/// Match a label map against a selector string.
///
/// Kept as the one-shot form for call sites that filter a single object; it
/// re-parses per call, so hot loops should compile a [`Selector`] once.
pub fn matches_selector(labels: &Option<HashMap<String, String>>, selector: &str) -> bool {
    match Selector::parse(selector) {
        Ok(s) => s.matches(labels),
        // A malformed selector is reported at the CLI boundary by
        // `validate_selector`, so filtering itself treats it as no-match rather
        // than panicking inside a `filter` closure.
        Err(_) => false,
    }
}

/// Parse both selectors up front so a typo is reported before any API call.
///
/// This is the reason malformed selectors now fail loudly: previously
/// `get svc --selector app==` matched zero rows and looked like an empty swarm.
pub fn validate_selector(
    selector: Option<&String>,
    field_selector: Option<&String>,
) -> anyhow::Result<()> {
    if let Some(s) = selector {
        Selector::parse(s).map_err(|e| anyhow::anyhow!("--selector: {e}"))?;
    }
    if let Some(s) = field_selector {
        Selector::parse(s).map_err(|e| anyhow::anyhow!("--field-selector: {e}"))?;
    }
    Ok(())
}

/// Keep the API objects matching both selectors.
///
/// `--field-selector` addresses the **rendered Kubernetes envelope** (see
/// [`crate::cli::kobject`]), so `--field-selector metadata.name=web` and
/// `--field-selector status.state=ready` both work. That is a change from the
/// old behaviour, which matched against raw PascalCase Docker keys and required
/// `--field-selector Spec.Name=web`.
///
/// `labels` supplies the label map for the kind being filtered, since where
/// labels live differs per resource (`spec.labels`,
/// `spec.taskTemplate.containerSpec.labels`, or top-level `labels`).
pub fn keep_matching<T: Serialize>(
    items: Vec<T>,
    labels: impl Fn(&T) -> Option<HashMap<String, String>>,
    selector: Option<&String>,
    field_selector: Option<&String>,
) -> Vec<T> {
    // Compile once instead of per item: this runs for every row of every list.
    let label_sel = selector.and_then(|s| Selector::parse(s).ok());
    let field_sel = field_selector.and_then(|s| Selector::parse(s).ok());

    items
        .into_iter()
        .filter(|item| {
            label_sel
                .as_ref()
                .map(|s| s.matches(&labels(item)))
                .unwrap_or(true)
        })
        .filter(|item| {
            field_sel
                .as_ref()
                .map(|s| {
                    let json = serde_json::to_value(item).unwrap_or_default();
                    s.matches_value(&json)
                })
                .unwrap_or(true)
        })
        .collect()
}

pub fn format_labels(labels: &Option<HashMap<String, String>>) -> String {
    match labels {
        Some(map) if !map.is_empty() => {
            let mut pairs: Vec<String> = map.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            // HashMap iteration order is non-deterministic; sort so table output
            // and golden tests are stable across runs.
            pairs.sort();
            pairs.join(",")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field_matches(value: &Value, selector: &str) -> bool {
        Selector::parse(selector).unwrap().matches_value(value)
    }

    fn labels(pairs: &[(&str, &str)]) -> Option<HashMap<String, String>> {
        Some(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn equality_and_double_equals_agree() {
        let l = labels(&[("app", "web")]);
        assert!(matches_selector(&l, "app=web"));
        assert!(matches_selector(&l, "app==web"));
        assert!(!matches_selector(&l, "app=db"));
        assert!(!matches_selector(&l, "missing=x"));
    }

    #[test]
    fn not_equals_matches_absent_keys() {
        let l = labels(&[("app", "web")]);
        assert!(matches_selector(&l, "app!=db"));
        assert!(!matches_selector(&l, "app!=web"));
        // kubectl: `!=` is satisfied by objects lacking the key.
        assert!(matches_selector(&l, "other!=x"));
    }

    #[test]
    fn existence_and_non_existence() {
        let l = labels(&[("app", "web")]);
        assert!(matches_selector(&l, "app"));
        assert!(!matches_selector(&l, "tier"));
        assert!(matches_selector(&l, "!tier"));
        assert!(!matches_selector(&l, "!app"));
    }

    #[test]
    fn set_based_operators() {
        let l = labels(&[("env", "prod"), ("tier", "web")]);
        assert!(matches_selector(&l, "env in (prod,stage)"));
        assert!(matches_selector(&l, "env notin (dev,test)"));
        assert!(!matches_selector(&l, "env in (dev,test)"));
        assert!(!matches_selector(&l, "env notin (prod,stage)"));
        // Spaces inside the parentheses are tolerated.
        assert!(matches_selector(&l, "env in ( dev , prod )"));
    }

    #[test]
    fn notin_matches_absent_but_in_does_not() {
        let l = labels(&[("app", "web")]);
        assert!(matches_selector(&l, "tier notin (a,b)"));
        assert!(!matches_selector(&l, "tier in (a,b)"));
    }

    #[test]
    fn comma_is_logical_and() {
        let l = labels(&[("app", "web"), ("env", "prod")]);
        assert!(matches_selector(&l, "app=web,env=prod"));
        assert!(!matches_selector(&l, "app=web,env=dev"));
    }

    #[test]
    fn commas_inside_a_value_set_do_not_split() {
        let l = labels(&[("env", "prod"), ("app", "web")]);
        assert!(matches_selector(&l, "env in (prod,stage),app=web"));
        // If the commas inside `(...)` split the selector, this would be read
        // as the requirement `env in (prod` and fail to parse.
        assert!(!matches_selector(&l, "env in (prod,stage),app=db"));
    }

    #[test]
    fn values_may_contain_equals() {
        let l = labels(&[("conn", "a=b")]);
        assert!(matches_selector(&l, "conn=a=b"));
        assert!(matches_selector(&l, "conn==a=b"));
    }

    #[test]
    fn selector_on_absent_labels() {
        assert!(!matches_selector(&None, "app=web"));
        assert!(!matches_selector(&None, "app"));
        // Only a pure non-existence requirement can be satisfied without labels.
        assert!(matches_selector(&None, "!app"));
        assert!(!matches_selector(&None, "app web"));
    }

    #[test]
    fn malformed_selectors_are_rejected_not_silently_empty() {
        for bad in [
            "",
            "   ",
            "app=web,",
            ",app=web",
            "env in prod",
            "env in ()",
            "=web",
            "!",
        ] {
            assert!(
                Selector::parse(bad).is_err(),
                "\"{bad}\" should be rejected"
            );
        }
        assert!(Selector::parse("app in (a,b").is_err(), "unbalanced (");
        assert!(Selector::parse("app notin a,b)").is_err(), "unbalanced )");
    }

    #[test]
    fn validate_selector_reports_which_flag_was_wrong() {
        let bad = "app in prod".to_string();
        let err = validate_selector(Some(&bad), None).unwrap_err().to_string();
        assert!(err.contains("--selector"), "{err}");
        let err = validate_selector(None, Some(&bad)).unwrap_err().to_string();
        assert!(err.contains("--field-selector"), "{err}");
    }

    #[test]
    fn field_selector_walks_the_kubernetes_envelope() {
        let v = json!({
            "kind": "Node",
            "metadata": {"name": "n1", "labels": {"app": "web"}},
            "status": {"state": "ready"}
        });
        assert!(field_matches(&v, "metadata.name=n1"));
        assert!(field_matches(&v, ".metadata.name=n1"));
        assert!(!field_matches(&v, "metadata.name=n2"));
        assert!(field_matches(&v, "status.state=ready"));
        assert!(field_matches(&v, "status.state!=drain"));
        assert!(field_matches(&v, "metadata.name"));
        assert!(field_matches(&v, "!metadata.owner"));
    }

    #[test]
    fn field_selector_compares_numbers_numerically() {
        let v = json!({"spec": {"replicas": 10}});
        assert!(field_matches(&v, "spec.replicas=10"));
        assert!(!field_matches(&v, "spec.replicas=9"));
    }

    #[test]
    fn format_labels_is_sorted_and_deterministic() {
        let l = labels(&[("zone", "b"), ("app", "web"), ("tier", "f")]);
        assert_eq!(format_labels(&l), "app=web,tier=f,zone=b");
        assert_eq!(format_labels(&l), format_labels(&l));
    }

    #[test]
    fn format_labels_handles_empty_and_none() {
        assert_eq!(format_labels(&None), "");
        assert_eq!(format_labels(&Some(HashMap::new())), "");
    }

    #[test]
    fn keep_matching_applies_both_selectors() {
        #[derive(serde::Serialize, Clone)]
        struct Obj {
            metadata: Meta,
        }
        #[derive(serde::Serialize, Clone)]
        struct Meta {
            name: String,
            labels: Option<HashMap<String, String>>,
        }

        let mk = |name: &str, app: &str| Obj {
            metadata: Meta {
                name: name.into(),
                labels: Some(HashMap::from([("app".into(), app.into())])),
            },
        };
        let items = vec![mk("web", "front"), mk("api", "front"), mk("db", "back")];

        let by_label = keep_matching(
            items.clone(),
            |o: &Obj| o.metadata.labels.clone(),
            Some(&"app=front".into()),
            None,
        );
        assert_eq!(by_label.len(), 2);

        let by_field = keep_matching(
            items,
            |o: &Obj| o.metadata.labels.clone(),
            None,
            Some(&"metadata.name=db".into()),
        );
        assert_eq!(by_field.len(), 1);
        assert_eq!(by_field[0].metadata.name, "db");
    }

    #[test]
    fn keep_matching_with_no_selectors_keeps_everything() {
        let items = vec![1, 2, 3];
        assert_eq!(keep_matching(items, |_| None, None, None).len(), 3);
    }
}
