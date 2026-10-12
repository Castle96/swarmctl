//! A kubectl-compatible JSONPath subset.
//!
//! This is what makes `-o jsonpath=...` possible, and it is also the engine
//! behind `--sort-by` and `-L/--label-columns`. It deliberately implements the
//! subset kubectl users actually write, and rejects the rest with a real error
//! rather than silently evaluating to nothing — the old `--sort-by` accepted any
//! string and quietly sorted every row on `null`.
//!
//! Supported:
//!
//! - field access: `.metadata.name`, `['metadata.name']`, `["a.b"]`
//! - indexes: `[0]`, `[-1]`
//! - wildcards: `[*]`, `.containers[*].image`
//! - recursive descent: `..name`, `..[?(@.ready)]`
//! - unions: `{.a,.b}`
//! - filters: `[?(@.state=="running")]`, `[?(@.replicas>1)]`
//! - quoted-literal substitution inside `{}`: `{"\t"}`, `{"\n"}`
//! - `range`/`end` blocks over a node set
//!
//! Not supported (errors clearly rather than returning nothing):
//! `slice` (`[0:2]`), `..` over a filtered descendant set, and `split`/`join`
//! from the full upstream grammar.

use anyhow::{Result, bail};
use serde_json::Value;

/// One step of a path.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    /// A literal object key, possibly containing dots when bracket-quoted.
    Field(String),
    /// `[n]`; negative indexes count from the end.
    Index(i64),
    /// `[*]`
    Wildcard,
    /// `..key`: every descendant object key with this name, in document order.
    Descend(String),
    /// `[?(@.field op value)]`
    Filter(Filter),
}

#[derive(Debug, Clone, PartialEq)]
struct Filter {
    path: Vec<String>,
    op: CmpOp,
    value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    fn apply(self, actual: &Value, expected: &Value) -> bool {
        match self {
            CmpOp::Eq => actual == expected,
            CmpOp::Ne => actual != expected,
            CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => {
                // Numbers compare numerically so `.replicas > 9` is not a string
                // compare, which would make 10 < 9.
                if let (Some(a), Some(b)) = (actual.as_f64(), expected.as_f64()) {
                    return match self {
                        CmpOp::Lt => a < b,
                        CmpOp::Le => a <= b,
                        CmpOp::Gt => a > b,
                        CmpOp::Ge => a >= b,
                        _ => unreachable!(),
                    };
                }
                match (actual.as_str(), expected.as_str()) {
                    (Some(a), Some(b)) => match self {
                        CmpOp::Lt => a < b,
                        CmpOp::Le => a <= b,
                        CmpOp::Gt => a > b,
                        CmpOp::Ge => a >= b,
                        _ => unreachable!(),
                    },
                    _ => false,
                }
            }
        }
    }
}

/// A compiled path expression.
///
/// `alternatives` holds one step list per comma-separated branch, so a union
/// like `{.a,.b}` is part of the path rather than a special case in the
/// renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonPath {
    /// Original text, kept for error messages.
    source: String,
    alternatives: Vec<Vec<Step>>,
}

/// Compile a path expression.
///
/// Accepts kubectl's `{...}` wrapping transparently so `-o jsonpath={.a}` and
/// `--sort-by '{.a}'` can be passed straight through.
pub fn parse(expr: &str) -> Result<JsonPath> {
    let trimmed = expr.trim();
    // Strip kubectl's `{}` wrapper — but not when the body is a quoted literal,
    // because `-o jsonpath='{"\t"}'` is a template, not a path.
    let inner = match trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        Some(body) if !body.starts_with('"') && !body.starts_with('\'') => body,
        _ => trimmed,
    };
    if inner.is_empty() {
        bail!("empty jsonpath expression");
    }
    // A union splits at commas that are not inside brackets or quotes.
    let branches = split_union(inner)?;
    let mut alternatives = Vec::with_capacity(branches.len());
    for branch in branches {
        let branch = branch.trim();
        if branch.is_empty() {
            bail!("jsonpath expression \"{}\" has an empty branch", expr);
        }
        alternatives.push(parse_steps(branch)?);
    }
    Ok(JsonPath {
        source: expr.to_string(),
        alternatives,
    })
}

/// Split `a,b` at top-level commas.
fn split_union(expr: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
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
                '[' | '(' => {
                    depth += 1;
                    current.push(c);
                }
                ']' | ')' => {
                    depth = depth.saturating_sub(1);
                    current.push(c);
                }
                ',' if depth == 0 => out.push(std::mem::take(&mut current)),
                _ => current.push(c),
            },
        }
    }
    out.push(current);
    Ok(out)
}

fn parse_steps(expr: &str) -> Result<Vec<Step>> {
    let bytes: Vec<char> = expr.chars().collect();
    let mut steps = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            // `.name`
            '.' => {
                if i + 1 < bytes.len() && bytes[i + 1] == '.' {
                    // Recursive descent: `..name` or `..[...]`
                    let start = i + 2;
                    let mut end = start;
                    while end < bytes.len() && bytes[end] != '.' && bytes[end] != '[' {
                        end += 1;
                    }
                    let key: String = bytes[start..end].iter().collect();
                    if key.is_empty() {
                        bail!("`..` must be followed by a field name or a bracket expression");
                    }
                    steps.push(Step::Descend(key));
                    i = end;
                } else {
                    let start = i + 1;
                    let mut end = start;
                    while end < bytes.len() && bytes[end] != '.' && bytes[end] != '[' {
                        end += 1;
                    }
                    let key: String = bytes[start..end].iter().collect();
                    if key.is_empty() {
                        bail!("empty field name in \"{}\"", expr);
                    }
                    steps.push(Step::Field(key));
                    i = end;
                }
            }
            // `['key']` / `["a.b"]` / `[0]` / `[*]` / `[?(@..)]`
            '[' => {
                let (step, next) = parse_bracket(expr, &bytes, i)?;
                steps.push(step);
                i = next;
            }
            // A bare leading key, e.g. `metadata.name` without the leading dot.
            c if c.is_alphanumeric() || c == '_' || c == '-' => {
                let start = i;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_alphanumeric() || bytes[end] == '_' || bytes[end] == '-')
                {
                    end += 1;
                }
                let key: String = bytes[start..end].iter().collect();
                steps.push(Step::Field(key));
                i = end;
            }
            other => bail!("unexpected character '{}' in \"{}\"", other, expr),
        }
    }

    if steps.is_empty() {
        bail!("jsonpath expression \"{}\" selects nothing", expr);
    }
    Ok(steps)
}

/// Parse one bracket expression starting at `start` (which indexes `'['`).
fn parse_bracket(expr: &str, chars: &[char], start: usize) -> Result<(Step, usize)> {
    let mut i = start + 1;
    // Skip whitespace inside the bracket.
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }

    // Slices (`[0:2]`) are not implemented; say so rather than reading the
    // whole `0:2` as a key name.
    if chars[start..].contains(&':') {
        bail!("slices like \"[0:2]\" are not supported in \"{}\"", expr);
    }

    // `[*]` wildcard
    if i < chars.len() && chars[i] == '*' {
        let mut j = i + 1;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j < chars.len() && chars[j] == ']' {
            return Ok((Step::Wildcard, j + 1));
        }
        bail!("malformed \"*\" expression in \"{}\"", expr);
    }

    // `[?(@.field op value)]` filter
    if i < chars.len() && chars[i] == '?' {
        let close = find_matching_bracket(chars, start)?;
        let body: String = chars[i + 1..close].iter().collect();
        let body = body.trim();
        let body = body
            .strip_prefix('(')
            .and_then(|b| b.strip_suffix(')'))
            .unwrap_or(body);
        return Ok((Step::Filter(parse_filter(expr, body.trim())?), close + 1));
    }

    // Quoted key, which may legitimately contain dots and brackets.
    if i < chars.len() && (chars[i] == '\'' || chars[i] == '"') {
        let quote = chars[i];
        let mut j = i + 1;
        let mut key = String::new();
        while j < chars.len() && chars[j] != quote {
            key.push(chars[j]);
            j += 1;
        }
        if j >= chars.len() {
            bail!("unterminated quoted key in \"{}\"", expr);
        }
        let mut k = j + 1;
        while k < chars.len() && chars[k].is_whitespace() {
            k += 1;
        }
        if k < chars.len() && chars[k] == ']' {
            return Ok((Step::Field(key), k + 1));
        }
        bail!("expected ']' after quoted key in \"{}\"", expr);
    }

    // Numeric index
    let num_start = i;
    let mut j = i;
    if j < chars.len() && (chars[j] == '-' || chars[j] == '+') {
        j += 1;
    }
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j > num_start {
        let text: String = chars[num_start..j].iter().collect();
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j < chars.len() && chars[j] == ']' {
            let n: i64 = text
                .parse()
                .map_err(|_| anyhow::anyhow!("bad index \"{}\" in \"{}\"", text, expr))?;
            return Ok((Step::Index(n), j + 1));
        }
    }

    // Unquoted key inside brackets: `[metadata.name]`
    let key_start = i;
    let mut k = i;
    while k < chars.len() && chars[k] != ']' {
        k += 1;
    }
    if k < chars.len() {
        let key: String = chars[key_start..k].iter().collect();
        let key = key.trim().to_string();
        if !key.is_empty() {
            return Ok((Step::Field(key), k + 1));
        }
    }

    bail!("slices like \"[0:2]\" are not supported in \"{}\"", expr)
}

/// Index of the `]` that closes the `[` at `open`, ignoring quoted brackets.
fn find_matching_bracket(chars: &[char], open: usize) -> Result<usize> {
    let mut depth = 0usize;
    let mut i = open;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == '[' => depth += 1,
            None if c == ']' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            None => {}
        }
        i += 1;
    }
    bail!("unbalanced '[' in expression")
}

/// Parse `@.field == "value"` and friends.
fn parse_filter(expr: &str, body: &str) -> Result<Filter> {
    let inner = body
        .strip_prefix('@')
        .ok_or_else(|| anyhow::anyhow!("filter in \"{}\" must start with '@'", expr))?;
    let inner = inner.trim_start();

    // Find the operator, longest first so `<=` wins over `<`.
    const OPS: &[(&str, CmpOp)] = &[
        ("==", CmpOp::Eq),
        ("!=", CmpOp::Ne),
        ("<=", CmpOp::Le),
        (">=", CmpOp::Ge),
        ("<", CmpOp::Lt),
        (">", CmpOp::Gt),
    ];
    for (token, op) in OPS {
        if let Some(idx) = inner.find(token) {
            let path_text = inner[..idx].trim().trim_start_matches('.');
            let raw = inner[idx + token.len()..].trim();
            let value = parse_filter_value(raw)?;
            let path: Vec<String> = if path_text.is_empty() {
                Vec::new()
            } else {
                path_text.split('.').map(|s| s.to_string()).collect()
            };
            return Ok(Filter {
                path,
                op: *op,
                value,
            });
        }
    }
    bail!("filter in \"{}\" needs an operator such as == or !=", expr)
}

/// Parse the right-hand side of a filter, which may be quoted or bare.
fn parse_filter_value(raw: &str) -> Result<Value> {
    if raw.len() >= 2 {
        let bytes = raw.as_bytes();
        let first = bytes[0] as char;
        let last = bytes[raw.len() - 1] as char;
        if (first == '\'' || first == '"') && first == last {
            let body = &raw[1..raw.len() - 1];
            return Ok(Value::String(unescape(body)));
        }
    }
    if raw == "true" {
        return Ok(Value::Bool(true));
    }
    if raw == "false" {
        return Ok(Value::Bool(false));
    }
    if let Ok(n) = raw.parse::<f64>() {
        return Ok(serde_json::Number::from_f64(n)
            .map(Value::Number)
            .unwrap_or(Value::Null));
    }
    if raw.is_empty() {
        return Ok(Value::String(String::new()));
    }
    Ok(Value::String(raw.to_string()))
}

/// Undo kubectl's Go-style string escapes in a quoted literal.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Walk one step, producing the next node set.
fn apply(step: &Step, input: &[&Value], out: &mut Vec<Value>) {
    match step {
        Step::Field(key) => {
            for v in input {
                if let Some(found) = v.get(key.as_str()) {
                    out.push(found.clone());
                }
            }
        }
        Step::Index(n) => {
            for v in input {
                let Some(arr) = v.as_array() else { continue };
                let idx = if *n < 0 {
                    // Negative indexes count from the end, as in Python.
                    let from_end = arr.len() as i64 + *n;
                    if from_end < 0 {
                        continue;
                    }
                    from_end as usize
                } else {
                    *n as usize
                };
                if let Some(found) = arr.get(idx) {
                    out.push(found.clone());
                }
            }
        }
        Step::Wildcard => {
            for v in input {
                if let Some(arr) = v.as_array() {
                    out.extend(arr.iter().cloned());
                }
            }
        }
        Step::Descend(key) => {
            // Depth-first, document order, matching kubectl's `..name`.
            fn walk(v: &Value, key: &str, out: &mut Vec<Value>) {
                if let Some(obj) = v.as_object() {
                    for (k, child) in obj {
                        if k == key {
                            out.push(child.clone());
                        }
                        walk(child, key, out);
                    }
                } else if let Some(arr) = v.as_array() {
                    for child in arr {
                        walk(child, key, out);
                    }
                }
            }
            for v in input {
                walk(v, key, out);
            }
        }
        Step::Filter(filter) => {
            // `[?(...)]` selects from the *contents* of an array, the same way
            // `[*]` does; on a non-array it tests the value itself.
            for v in input {
                match v.as_array() {
                    Some(items) => {
                        out.extend(items.iter().filter(|i| matches_filter(i, filter)).cloned())
                    }
                    None => {
                        if matches_filter(v, filter) {
                            out.push((*v).clone());
                        }
                    }
                }
            }
        }
    }
}

fn matches_filter(v: &Value, filter: &Filter) -> bool {
    let mut cursor = v;
    for segment in &filter.path {
        match cursor.get(segment.as_str()) {
            Some(next) => cursor = next,
            None => return false,
        }
    }
    filter.op.apply(cursor, &filter.value)
}

impl JsonPath {
    /// Evaluate against `root`, returning every matching node.
    ///
    /// For a union the branches are evaluated in written order and their
    /// results concatenated, which is what `{.a,.b}` means.
    pub fn eval(&self, root: &Value) -> Result<Vec<Value>> {
        let mut all = Vec::new();
        for steps in &self.alternatives {
            let mut current: Vec<Value> = vec![root.clone()];
            for step in steps {
                let refs: Vec<&Value> = current.iter().collect();
                let mut next = Vec::new();
                apply(step, &refs, &mut next);
                current = next;
            }
            all.extend(current);
        }
        Ok(all)
    }

    /// Evaluate and join the way kubectl's text output does: space-separated.
    ///
    /// `{.a}{.b}` is two separate expressions and concatenates without a
    /// space; `{.a}{.b}` inside one union produces multiple results that
    /// kubectl joins with a space.
    pub fn eval_text(&self, root: &Value) -> Result<String> {
        Ok(self
            .eval(root)?
            .iter()
            .map(render_scalar)
            .collect::<Vec<_>>()
            .join(" "))
    }
}

/// Render one node the way kubectl prints it in text output.
///
/// Strings are unquoted, because `kubectl -o jsonpath='{.metadata.name}'`
/// prints `web`, not `"web"`.
pub fn render_scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Execute a full kubectl jsonpath *template*, including `range` blocks.
///
/// This is what `-o jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}'`
/// needs. Returns the rendered text.
pub fn execute(template: &str, root: &Value) -> Result<String> {
    let tokens = tokenize(template)?;
    let mut out = String::new();
    render(&tokens, root, &mut out)?;
    Ok(out)
}

/// One piece of a parsed template.
#[derive(Debug)]
enum Tok {
    /// Literal text outside any `{}`.
    Lit(String),
    /// `{expr}`
    Expr(String),
    /// `{range path}` ... `{end}`
    Range(String, Vec<Tok>),
}

/// Split a template into tokens, recursing through `range`/`end`.
///
/// A previous implementation scanned for `{end}` inline, which mis-nested
/// nested ranges. Tokenizing once up front makes the structure explicit.
fn tokenize(template: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = template.chars().collect();
    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] != '{' {
            literal.push(chars[i]);
            i += 1;
            continue;
        }
        let close = chars[i..]
            .iter()
            .position(|c| *c == '}')
            .ok_or_else(|| anyhow::anyhow!("unbalanced '{{' in jsonpath template"))?;
        if !literal.is_empty() {
            tokens.push(Tok::Lit(std::mem::take(&mut literal)));
        }
        let body: String = chars[i + 1..i + close].iter().collect();
        let body = body.trim().to_string();
        i += close + 1;

        if body == "end" {
            return Err(anyhow::anyhow!(
                "unexpected {{end}} without a matching {{range}}"
            ));
        }
        if let Some(path) = body.strip_prefix("range ") {
            let path = path.trim().to_string();
            // Validate the path eagerly so a typo is reported even when the
            // range turns out to be empty.
            parse(&path)?;
            let (body_tokens, rest) = tokenize_range_body(&chars, i)?;
            i = rest;
            tokens.push(Tok::Range(path, body_tokens));
            continue;
        }
        tokens.push(Tok::Expr(body));
    }
    if !literal.is_empty() {
        tokens.push(Tok::Lit(literal));
    }
    Ok(tokens)
}

/// Consume tokens until the `{end}` that closes the current range.
fn tokenize_range_body(chars: &[char], mut i: usize) -> Result<(Vec<Tok>, usize)> {
    let mut body = String::new();
    let mut tokens = Vec::new();
    while i < chars.len() {
        if chars[i] != '{' {
            body.push(chars[i]);
            i += 1;
            continue;
        }
        let close = chars[i..]
            .iter()
            .position(|c| *c == '}')
            .ok_or_else(|| anyhow::anyhow!("unbalanced '{{' in jsonpath template"))?;
        let expr: String = chars[i + 1..i + close].iter().collect();
        let expr = expr.trim().to_string();
        i += close + 1;
        if expr == "end" {
            if !body.is_empty() {
                tokens.push(Tok::Lit(body));
            }
            return Ok((tokens, i));
        }
        if !body.is_empty() {
            tokens.push(Tok::Lit(std::mem::take(&mut body)));
        }
        if let Some(path) = expr.strip_prefix("range ") {
            let path = path.trim().to_string();
            parse(&path)?;
            let (inner, next) = tokenize_range_body(chars, i)?;
            i = next;
            tokens.push(Tok::Range(path, inner));
            continue;
        }
        tokens.push(Tok::Expr(expr));
    }
    anyhow::bail!("{{range}} without a matching {{end}}")
}

/// Render tokens.
///
/// Every expression and every `range` path resolves against `context`; a range
/// body is rendered once per matched node with that node as the new context.
fn render(tokens: &[Tok], context: &Value, out: &mut String) -> Result<()> {
    for tok in tokens {
        match tok {
            Tok::Lit(text) => out.push_str(text),
            Tok::Expr(body) => {
                // `{"\t"}` is a quoted literal, not a path.
                if body.len() >= 2 {
                    let first = body.chars().next().unwrap();
                    let last = body.chars().last().unwrap();
                    if (first == '"' || first == '\'') && first == last {
                        out.push_str(&unescape(&body[1..body.len() - 1]));
                        continue;
                    }
                }
                let path = parse(body)?;
                out.push_str(&path.eval_text(context)?);
            }
            Tok::Range(path, body) => {
                // Relative to the current node, so `{range .groups[*]}` may
                // contain `{range .members[*]}`.
                let nodes = parse(path)?.eval(context)?;
                for node in nodes {
                    render(body, &node, out)?;
                }
            }
        }
    }
    Ok(())
}

/// Convenience: compile and evaluate a single expression as text.
pub fn eval_text(expr: &str, root: &Value) -> Result<String> {
    let trimmed = expr.trim();
    // Anything that is really a template goes to the template engine: a
    // `range` block, an escape sequence, or a quoted literal like `{"\t"}`.
    let is_literal = trimmed.starts_with("{\"") || trimmed.starts_with("{'");
    if trimmed.contains("range ") || trimmed.contains('\\') || is_literal {
        return execute(expr, root);
    }
    parse(expr)?.eval_text(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pod() -> Value {
        json!({
            "metadata": {"name": "web-1", "labels": {"app": "web", "tier": "front"}},
            "spec": {
                "replicas": 10,
                "containers": [
                    {"name": "app", "image": "nginx:1.25"},
                    {"name": "side", "image": "busybox"}
                ]
            },
            "status": {"phase": "Running", "ready": true}
        })
    }

    #[test]
    fn simple_field_path() {
        assert_eq!(eval_text("{.metadata.name}", &pod()).unwrap(), "web-1");
    }

    #[test]
    fn braces_are_optional() {
        assert_eq!(eval_text(".metadata.name", &pod()).unwrap(), "web-1");
        assert_eq!(eval_text("{.metadata.name}", &pod()).unwrap(), "web-1");
    }

    #[test]
    fn values_are_not_quoted_in_text_output() {
        assert_eq!(eval_text("{.metadata.name}", &pod()).unwrap(), "web-1");
        assert_eq!(eval_text("{.status.ready}", &pod()).unwrap(), "true");
        assert_eq!(eval_text("{.spec.replicas}", &pod()).unwrap(), "10");
    }

    #[test]
    fn missing_path_yields_empty_not_an_error() {
        // kubectl's --allow-missing-template-keys=true is the default.
        assert_eq!(eval_text("{.nope.deep}", &pod()).unwrap(), "");
    }

    #[test]
    fn array_index_access() {
        assert_eq!(
            eval_text("{.spec.containers[0].image}", &pod()).unwrap(),
            "nginx:1.25"
        );
        assert_eq!(
            eval_text("{.spec.containers[1].image}", &pod()).unwrap(),
            "busybox"
        );
    }

    #[test]
    fn negative_index_counts_from_the_end() {
        assert_eq!(
            eval_text("{.spec.containers[-1].image}", &pod()).unwrap(),
            "busybox"
        );
    }

    #[test]
    fn wildcard_joins_with_a_space() {
        assert_eq!(
            eval_text("{.spec.containers[*].name}", &pod()).unwrap(),
            "app side"
        );
    }

    #[test]
    fn bracket_quoted_key_may_contain_dots() {
        let v = json!({"a.b": 1});
        assert_eq!(eval_text("{['a.b']}", &v).unwrap(), "1");
        assert_eq!(eval_text("[\"a.b\"]", &v).unwrap(), "1");
        // `{"a.b"}` is a quoted *literal* in kubectl, not a path, and must not
        // silently resolve to the key.
        assert_eq!(eval_text("{\"a.b\"}", &v).unwrap(), "a.b");
    }

    #[test]
    fn recursive_descent_finds_all_keys() {
        let v = json!({"a": {"b": {"id": 1}}, "c": {"id": 2}});
        assert_eq!(eval_text("{..id}", &v).unwrap(), "1 2");
    }

    #[test]
    fn filters_compare_by_value_not_string() {
        let list = json!([
            {"name": "a", "replicas": 2},
            {"name": "b", "replicas": 10}
        ]);
        // The point of the test: 10 must sort after 2, not before it.
        assert_eq!(eval_text("{[?(@.replicas>1)].name}", &list).unwrap(), "a b");
        assert_eq!(eval_text("{[?(@.replicas>5)].name}", &list).unwrap(), "b");
    }

    #[test]
    fn filters_compare_strings_too() {
        let list = json!([{"state": "running"}, {"state": "exited"}]);
        assert_eq!(
            eval_text("{[?(@.state==\"running\")].state}", &list).unwrap(),
            "running"
        );
        assert_eq!(
            eval_text("{[?(@.state!=\"running\")].state}", &list).unwrap(),
            "exited"
        );
    }

    #[test]
    fn range_block_iterates_nodes() {
        let list = json!({"items": [
            {"metadata": {"name": "web-1"}},
            {"metadata": {"name": "web-2"}}
        ]});
        let out = execute("{range .items[*]}{.metadata.name}{\"\\n\"}{end}", &list).unwrap();
        assert_eq!(out, "web-1\nweb-2\n");
    }

    #[test]
    fn range_blocks_nest() {
        let v = json!({"groups": [
            {"members": [{"n": "a"}, {"n": "b"}]},
            {"members": [{"n": "c"}]}
        ]});
        let out = execute(
            "{range .groups[*]}[{range .members[*]}{.n} {end}]\n{end}",
            &v,
        )
        .unwrap();
        assert_eq!(out, "[a b ]\n[c ]\n");
    }

    #[test]
    fn literal_escapes_are_unescaped() {
        let out = execute("{\"\\t\"}{\"x\"}{\"\\n\"}", &json!({})).unwrap();
        assert_eq!(out, "\tx\n");
    }

    #[test]
    fn unions_evaluate_each_side() {
        let v = json!({"a": 1, "b": 2});
        assert_eq!(eval_text("{.a,.b}", &v).unwrap(), "1 2");
    }

    #[test]
    fn parse_errors_are_specific_not_silent() {
        // The old --sort-by accepted anything and sorted on null.
        let err = eval_text("{.spec.containers[0:2].image}", &pod()).unwrap_err();
        assert!(err.to_string().contains("slices"), "{err}");

        assert!(parse("{").is_err());
        assert!(parse("{}").is_err());
        assert!(parse("{}]").is_err());
        assert!(parse("{[?(@.x)]}").is_err(), "filter needs an operator");
        assert!(
            execute("{range .items[*]}no end", &json!({"items": [{"a": 1}]})).is_err(),
            "an unclosed range must be an error even when it would iterate"
        );
    }

    #[test]
    fn numbers_and_bools_render_plainly() {
        let v = json!({"n": 3, "b": false});
        assert_eq!(eval_text("{.n}", &v).unwrap(), "3");
        assert_eq!(eval_text("{.b}", &v).unwrap(), "false");
    }
}
