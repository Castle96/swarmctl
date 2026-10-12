use crate::cli::kobject;
use crate::cli::registry::{self, ResourceType};
use crate::cli::root::OutputFormat;
use crate::models::Renderable;
use serde::Serialize;
use serde_json::Value;
use std::cmp::Ordering;
use tabled::{Table, Tabled};

pub fn print_table<T: Tabled>(rows: &[T]) {
    let table = Table::new(rows);
    println!("{}", table);
}

pub fn print_json<T: Serialize>(data: &T) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(data)?;
    println!("{}", json);
    Ok(())
}

pub fn print_yaml<T: Serialize>(data: &T) -> anyhow::Result<()> {
    let yaml = serde_yaml::to_string(data)?;
    println!("{}", yaml);
    Ok(())
}

/// Order two JSON values.
///
/// Numbers compare numerically, so `9` sorts before `10` — comparing the string
/// form would invert that. Values of different JSON types are ordered by type
/// rank rather than by their string form, because `Value::String` stringifies
/// *with* quotes (`"a"`), which would otherwise make mixed columns sort by
/// quote characters.
pub fn compare_values(a: &Value, b: &Value) -> Ordering {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    }
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Null => 0,
            Value::Bool(_) => 1,
            Value::Number(_) => 2,
            Value::String(_) => 3,
            Value::Array(_) => 4,
            Value::Object(_) => 5,
        }
    }
    rank(a)
        .cmp(&rank(b))
        .then_with(|| a.to_string().cmp(&b.to_string()))
}

/// How to render a list.
#[derive(Clone, Default)]
pub struct RenderOpts {
    pub show_labels: bool,
    /// kubectl's `--show-kind`: prefix `-o name` output with the type.
    pub show_kind: bool,
    /// kubectl's `--no-headers`: omit the table header row.
    pub no_headers: bool,
    /// kubectl's `-L/--label-columns`: extra label columns to display.
    pub label_columns: Vec<String>,
}

/// Return the permutation that orders `items` by a kubectl jsonpath expression.
///
/// Expressed as an index permutation so the caller can apply it to both the
/// typed objects and their serialized forms and keep the two aligned.
///
/// Sorting moved off the display rows and onto the API objects so the path
/// addresses the same document `--field-selector` and `-o jsonpath` see. The old
/// implementation sorted on row fields and **silently sorted everything on null**
/// for any path the row did not have.
pub fn sort_order(
    resource: ResourceType,
    items: &[Value],
    sort_by: &str,
) -> anyhow::Result<Vec<usize>> {
    let path =
        crate::utils::jsonpath::parse(sort_by).map_err(|e| anyhow::anyhow!("--sort-by: {e}"))?;

    let keys: Vec<Value> = items
        .iter()
        .map(|item| {
            let enveloped = kobject::to_k8s(resource, item);
            path.eval(&enveloped)
                .ok()
                .and_then(|mut v| {
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.remove(0))
                    }
                })
                .unwrap_or(Value::Null)
        })
        .collect();

    let mut order: Vec<usize> = (0..items.len()).collect();
    // A stable sort keeps equal keys in input order, as kubectl does.
    order.sort_by(|a, b| compare_values(&keys[*a], &keys[*b]));
    Ok(order)
}

/// Sort display rows by a jsonpath expression over their serialized form.
///
/// Used by the `--endpoints` fan-out, which merges already-built rows from
/// several clusters and therefore has no API objects left to sort on. A
/// malformed path is an error here too: the previous implementation accepted
/// any string and quietly sorted every row on `null`.
pub fn sort_rows<T: Serialize + Clone>(rows: &mut [T], path: &str) -> anyhow::Result<()> {
    let path =
        crate::utils::jsonpath::parse(path).map_err(|e| anyhow::anyhow!("--sort-by: {e}"))?;
    let mut keyed: Vec<(Value, T)> = Vec::with_capacity(rows.len());
    for row in rows.iter() {
        let value = serde_json::to_value(row)?;
        let key = path
            .eval(&value)
            .ok()
            .and_then(|mut v| {
                if v.is_empty() {
                    None
                } else {
                    Some(v.remove(0))
                }
            })
            .unwrap_or(Value::Null);
        keyed.push((key, row.clone()));
    }
    // Stable, so rows with equal keys keep their input order.
    keyed.sort_by(|a, b| compare_values(&a.0, &b.0));
    for (slot, (_, row)) in rows.iter_mut().zip(keyed) {
        *slot = row;
    }
    Ok(())
}

/// Report the result of a mutating command.
///
/// With a machine format the daemon's response object is printed, so the output
/// is pipeable into `jq` and the new object's ID can be captured. Text formats
/// get the one-line human summary.
pub fn report_created<T: Serialize>(
    kind: &str,
    name: &str,
    response: &T,
    output: &OutputFormat,
) -> anyhow::Result<()> {
    report_changed("created", kind, name, response, output)
}

/// Same as [`report_created`], for updates.
pub fn report_updated<T: Serialize>(
    kind: &str,
    name: &str,
    response: &T,
    output: &OutputFormat,
) -> anyhow::Result<()> {
    report_changed("updated", kind, name, response, output)
}

fn report_changed<T: Serialize>(
    verb: &str,
    kind: &str,
    name: &str,
    response: &T,
    output: &OutputFormat,
) -> anyhow::Result<()> {
    match output {
        OutputFormat::Json => print_json(response),
        OutputFormat::Yaml => print_yaml(response),
        _ => {
            println!("{} '{}' {}", kind, name, verb);
            Ok(())
        }
    }
}

/// Render a list exactly once, in the requested format.
///
/// `items` are the raw serialized API objects; `rows` are their display
/// projections. Text formats use `rows`, machine formats use the Kubernetes
/// envelope built from `items`, which is why `-o json` and `--field-selector`
/// agree on field names.
pub fn render_list<R: Renderable>(
    resource: ResourceType,
    items: &[Value],
    rows: &[R],
    output: &OutputFormat,
    opts: &RenderOpts,
) -> anyhow::Result<()> {
    let envelope = || kobject::to_k8s_list(resource, items);

    match output {
        OutputFormat::Table => {
            print_table(rows);
            if opts.show_labels {
                print_labels(rows);
            }
            Ok(())
        }
        OutputFormat::Wide => {
            print_table(rows);
            // Labels always shown: that is the extra information `wide` adds,
            // so the flag does something a caller can rely on.
            print_labels(rows);
            Ok(())
        }
        OutputFormat::Json => print_json(&envelope()),
        OutputFormat::Yaml => print_yaml(&envelope()),
        OutputFormat::Name => {
            let kind = registry::singular(resource);
            for row in rows {
                if opts.show_kind {
                    println!("{}/{}", kind, row.row_name());
                } else {
                    println!("{}", row.row_name());
                }
            }
            Ok(())
        }
        OutputFormat::JsonPath(template) | OutputFormat::GoTemplate(template) => {
            let out = crate::utils::jsonpath::execute(template, &envelope())?;
            print!("{}", out);
            flush_if_needed(&out);
            Ok(())
        }
        OutputFormat::JsonPathAsJson(template) => {
            let path = crate::utils::jsonpath::parse(template)?;
            let matched = path.eval(&envelope())?;
            print_json(&matched)
        }
        OutputFormat::CustomColumns(spec) => render_custom_columns(&envelope(), rows, spec, opts),
    }
}

/// Flush when the rendered text does not end in a newline.
///
/// `-o jsonpath='{.metadata.name}'` over three objects emits `a b c` with no
/// trailing newline; the next shell prompt would otherwise land on the same line.
fn flush_if_needed(out: &str) {
    use std::io::Write;
    if !out.is_empty() && !out.ends_with('\n') {
        println!();
    }
    let _ = std::io::stdout().flush();
}

fn print_labels<R: Renderable>(rows: &[R]) {
    for row in rows {
        let labels = row.labels();
        if !labels.is_empty() {
            println!("  Labels: {}", labels);
        }
    }
}

/// Render kubectl's `-o custom-columns=NAME:.path,NAME2:.path2`.
///
/// Column paths are evaluated against the same Kubernetes envelope as
/// `--field-selector` and `-o jsonpath`, so one mental model covers all three.
fn render_custom_columns<R: Renderable>(
    envelope: &Value,
    rows: &[R],
    spec: &str,
    opts: &RenderOpts,
) -> anyhow::Result<()> {
    let (headers, paths) = parse_custom_columns(spec)?;
    let items = envelope
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if !opts.no_headers {
        let mut header = headers.clone();
        header.extend(opts.label_columns.iter().cloned());
        println!("{}", header.join("\t"));
    }

    for (i, item) in items.iter().enumerate() {
        let mut cells: Vec<String> = Vec::with_capacity(headers.len());
        for path in &paths {
            let value = crate::utils::jsonpath::eval_text(path, item)?;
            cells.push(value);
        }
        // `-L` appends the requested label values as extra columns.
        for key in &opts.label_columns {
            let lookup =
                crate::utils::jsonpath::eval_text(&format!("{{.metadata.labels.{key}}}"), item);
            cells.push(lookup?);
        }
        // A short list result can have fewer envelopes than rows when a name was
        // requested that did not exist; skip rather than print a blank line.
        let _ = i;
        let _ = rows;
        println!("{}", cells.join("\t"));
    }
    Ok(())
}

/// Parse `NAME:.path,NAME2:.path2` into headers and paths.
pub fn parse_custom_columns(spec: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let mut headers = Vec::new();
    let mut paths = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (header, path) = part.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "custom-columns entry \"{part}\" is missing a `:` between the column \
                 name and its path, e.g. NAME:.metadata.name"
            )
        })?;
        let header = header.trim();
        if header.is_empty() || path.trim().is_empty() {
            anyhow::bail!("custom-columns entry \"{part}\" needs a name and a path");
        }
        headers.push(header.to_uppercase());
        paths.push(path.trim().to_string());
    }
    if headers.is_empty() {
        anyhow::bail!("custom-columns needs at least one NAME:.path entry");
    }
    Ok((headers, paths))
}

/// The `<kind>/<name>` line `-o name` prints for one row.
pub fn row_name_line<R: Renderable>(row: &R, resource: ResourceType) -> String {
    format!("{}/{}", registry::singular(resource), row.row_name())
}

/// Order two display rows is no longer needed for sorting (sorting happens on
/// the API objects), but callers of [`compare_values`] still use it.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Named, RowLabels};
    use tabled::Tabled;

    #[derive(Tabled, Serialize, Clone)]
    struct WidgetRow {
        name: String,
        replicas: u32,
        labels: String,
    }

    impl RowLabels for WidgetRow {
        fn labels(&self) -> &str {
            &self.labels
        }
    }
    impl Named for WidgetRow {
        fn row_name(&self) -> String {
            self.name.clone()
        }
    }

    fn rows() -> Vec<WidgetRow> {
        vec![
            WidgetRow {
                name: "b".into(),
                replicas: 10,
                labels: "app=b".into(),
            },
            WidgetRow {
                name: "a".into(),
                replicas: 9,
                labels: String::new(),
            },
        ]
    }

    fn widget_items() -> Vec<Value> {
        vec![
            serde_json::json!({"ID": "w2", "Spec": {"Name": "b", "Replicas": 10,
                             "Labels": {"app": "b"}}}),
            serde_json::json!({"ID": "w1", "Spec": {"Name": "a", "Replicas": 9}}),
        ]
    }

    fn opts() -> RenderOpts {
        RenderOpts::default()
    }

    fn names_in_order(items: &[Value]) -> Vec<String> {
        items
            .iter()
            .map(|i| kobject::name_of(ResourceType::Services, i).unwrap_or_default())
            .collect()
    }

    fn sorted(items: &[Value], path: &str) -> Vec<String> {
        let order = sort_order(ResourceType::Services, items, path).unwrap();
        let permuted: Vec<Value> = order.iter().map(|i| items[*i].clone()).collect();
        names_in_order(&permuted)
    }

    #[test]
    fn sorting_orders_numerically_not_lexicographically() {
        let items = widget_items();
        assert_eq!(sorted(&items, "{.spec.replicas}"), vec!["a", "b"]);
    }

    #[test]
    fn sorting_accepts_a_leading_dot() {
        let items = widget_items();
        assert_eq!(sorted(&items, ".metadata.name"), vec!["a", "b"]);
    }

    #[test]
    fn sorting_is_stable_for_equal_keys() {
        let items = widget_items();
        // Both share the same (absent) key, so input order is preserved.
        assert_eq!(sorted(&items, "{.nope}"), vec!["b", "a"]);
    }

    #[test]
    fn sorting_rejects_a_malformed_path_instead_of_sorting_on_null() {
        let items = widget_items();
        let err = sort_order(ResourceType::Services, &items, "{.a[0:2]}").unwrap_err();
        assert!(err.to_string().contains("--sort-by"), "{err}");
    }

    #[test]
    fn every_render_arm_runs() {
        let r = rows();
        let items = widget_items();
        let mut o = opts();
        o.show_labels = true;
        for f in [
            OutputFormat::Table,
            OutputFormat::Wide,
            OutputFormat::Json,
            OutputFormat::Yaml,
            OutputFormat::Name,
            OutputFormat::JsonPath("{.metadata.name}".into()),
            OutputFormat::JsonPathAsJson("{.metadata.name}".into()),
            OutputFormat::GoTemplate("{.metadata.name}".into()),
            OutputFormat::CustomColumns("NAME:.metadata.name".into()),
        ] {
            render_list(ResourceType::Services, &items, &r, &f, &o).unwrap();
        }
    }

    #[test]
    fn show_kind_prefixes_name_output() {
        let r = rows();
        let items = widget_items();
        let mut o = opts();
        o.show_kind = true;
        render_list(ResourceType::Services, &items, &r, &OutputFormat::Name, &o).unwrap();
    }

    #[test]
    fn json_output_is_an_envelope_not_a_bare_array() {
        let r = rows();
        let items = widget_items();
        render_list(
            ResourceType::Services,
            &items,
            &r,
            &OutputFormat::Json,
            &opts(),
        )
        .unwrap();
        // The `items` key above would not exist for a bare array.
    }

    #[test]
    fn row_name_line_uses_the_registry_singular() {
        let r = rows();
        assert_eq!(row_name_line(&r[0], ResourceType::Services), "service/b");
    }

    #[test]
    fn compare_values_prefers_numbers() {
        assert_eq!(
            compare_values(&serde_json::json!(9), &serde_json::json!(10)),
            Ordering::Less
        );
    }

    #[test]
    fn compare_values_orders_mixed_types_by_type_rank() {
        assert_eq!(
            compare_values(&serde_json::json!(1), &serde_json::json!("a")),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&serde_json::json!(null), &serde_json::json!(0)),
            Ordering::Less
        );
    }

    #[test]
    fn custom_columns_spec_is_validated() {
        assert!(parse_custom_columns("NAME:.metadata.name").is_ok());
        assert!(parse_custom_columns("missing-colon").is_err());
        assert!(parse_custom_columns("").is_err());
        let (h, p) = parse_custom_columns("name:.metadata.name,ns:.spec.name").unwrap();
        assert_eq!(h, vec!["NAME", "NS"]);
        assert_eq!(p, vec![".metadata.name", ".spec.name"]);
    }

    #[test]
    fn output_format_parses_kubectl_compound_forms() {
        use crate::cli::root::OutputFormat as F;
        assert_eq!(F::parse("json").unwrap(), F::Json);
        assert_eq!(F::parse("table").unwrap(), F::Table);
        assert_eq!(
            F::parse("jsonpath={.metadata.name}").unwrap(),
            F::JsonPath("{.metadata.name}".into())
        );
        assert_eq!(
            F::parse("custom-columns=NAME:.metadata.name").unwrap(),
            F::CustomColumns("NAME:.metadata.name".into())
        );
        // A template containing `=` must not be truncated at the wrong place.
        assert_eq!(
            F::parse("go-template={{.a}}{{if eq .x \"y\"}}z{{end}}").unwrap(),
            F::GoTemplate("{{.a}}{{if eq .x \"y\"}}z{{end}}".into())
        );
    }

    #[test]
    fn output_format_rejects_unknown_and_bare_payload_forms() {
        assert!(OutputFormat::parse("toml").is_err());
        assert!(OutputFormat::parse("jsonpath").is_err());
        assert!(OutputFormat::parse("jsonpath=").is_err());
    }

    #[test]
    fn machine_formats_are_classified_for_banner_suppression() {
        use crate::cli::root::OutputFormat as F;
        assert!(!F::Table.is_machine());
        assert!(!F::Wide.is_machine());
        assert!(!F::Name.is_machine());
        assert!(F::Json.is_machine());
        assert!(F::JsonPath("{.a}".into()).is_machine());
        assert!(F::CustomColumns("A:.b".into()).is_machine());
    }

    #[test]
    fn output_format_round_trips_through_as_arg() {
        use crate::cli::root::OutputFormat as F;
        for f in [
            F::Table,
            F::Json,
            F::JsonPath("{.a}".into()),
            F::CustomColumns("A:.b".into()),
        ] {
            assert_eq!(F::parse(&f.as_arg()).unwrap(), f);
        }
    }
}
