use crate::models::Renderable;
use serde::Serialize;
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

/// Build a JSON pointer from a dotted field path.
///
/// Accepts both `a.b.c` and kubectl's leading-dot form `.a.b.c`, which would
/// otherwise produce the invalid pointer `//a/b/c` and silently match nothing.
pub fn json_pointer(path: &str) -> String {
    format!("/{}", path.trim_start_matches('.').replace('.', "/"))
}

/// Order two JSON values.
///
/// Numbers compare numerically, so `9` sorts before `10` — comparing the string
/// form would invert that. Values of different JSON types are ordered by type
/// rank rather than by their string form, because `Value::String` stringifies
/// *with* quotes (`"a"`), which would otherwise make mixed columns sort by
/// quote characters.
pub fn compare_values(a: &serde_json::Value, b: &serde_json::Value) -> Ordering {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    }
    fn rank(v: &serde_json::Value) -> u8 {
        match v {
            serde_json::Value::Null => 0,
            serde_json::Value::Bool(_) => 1,
            serde_json::Value::Number(_) => 2,
            serde_json::Value::String(_) => 3,
            serde_json::Value::Array(_) => 4,
            serde_json::Value::Object(_) => 5,
        }
    }
    rank(a)
        .cmp(&rank(b))
        .then_with(|| a.to_string().cmp(&b.to_string()))
}

/// Sort rows in place by a dotted path into each row's serialized form.
///
/// The path addresses row fields, e.g. `--sort-by replicas` or `.name`.
/// Accepts an optional leading dot, since kubectl-style paths are written that
/// way and a leading dot would otherwise produce an invalid JSON pointer.
pub fn sort_rows<T: Serialize + Clone>(rows: &mut Vec<T>, path: &str) -> anyhow::Result<()> {
    let pointer = json_pointer(path);
    let mut keyed: Vec<(serde_json::Value, T)> = Vec::with_capacity(rows.len());
    for row in rows.drain(..) {
        let value = serde_json::to_value(&row)?;
        let key = value
            .pointer(&pointer)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        keyed.push((key, row));
    }
    // Stable, so rows with equal keys keep their input order.
    keyed.sort_by(|a, b| compare_values(&a.0, &b.0));
    *rows = keyed.into_iter().map(|(_, row)| row).collect();
    Ok(())
}

/// Report the result of a mutating command.
///
/// With `-o json`/`-o yaml` the daemon's response object is printed, so the
/// output is pipeable into `jq` and the new object's ID can be captured. Text
/// formats get the one-line human summary.
pub fn report_created<T: Serialize>(
    kind: &str,
    name: &str,
    response: &T,
    output: &crate::cli::root::OutputFormat,
) -> anyhow::Result<()> {
    use crate::cli::root::OutputFormat;
    match output {
        OutputFormat::Json => print_json(response),
        OutputFormat::Yaml => print_yaml(response),
        _ => {
            println!("{} '{}' created", kind, name);
            Ok(())
        }
    }
}

/// Same as [`report_created`], for updates.
pub fn report_updated<T: Serialize>(
    kind: &str,
    name: &str,
    response: &T,
    output: &crate::cli::root::OutputFormat,
) -> anyhow::Result<()> {
    use crate::cli::root::OutputFormat;
    match output {
        OutputFormat::Json => print_json(response),
        OutputFormat::Yaml => print_yaml(response),
        _ => {
            println!("{} '{}' updated", kind, name);
            Ok(())
        }
    }
}

/// Render rows exactly once, in the requested format.
///
/// Sorting happens *before* rendering. Previously each list handler rendered
/// first and then, if `--sort-by` was given, serialized and printed the rows a
/// second time — so `get services -o json --sort-by name` emitted two
/// concatenated JSON documents and `get nodes --sort-by hostname` printed a
/// table followed by a JSON array.
pub fn render_rows<T: Renderable>(
    rows: &[T],
    output: &crate::cli::root::OutputFormat,
    show_labels: bool,
) -> anyhow::Result<()> {
    use crate::cli::root::OutputFormat;

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            print_table(rows);
            if show_labels {
                for row in rows {
                    if !row.labels().is_empty() {
                        println!("  Labels: {}", row.labels());
                    }
                }
            }
        }
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Name => {
            let kind = row_kind::<T>();
            for row in rows {
                println!("{}/{}", kind, row.row_name());
            }
        }
    }
    Ok(())
}

/// Singular resource noun used by `-o name` (e.g. `node/name1`).
///
/// Derived from the struct name so a new row type cannot forget to declare it.
fn row_kind<T: Renderable>() -> String {
    std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .and_then(|s| s.strip_suffix("Row"))
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "resource".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Named, RowLabels};
    use serde::Serialize;
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

    #[test]
    fn sort_rows_orders_numerically_not_lexicographically() {
        let mut r = rows();
        sort_rows(&mut r, "replicas").unwrap();
        assert_eq!(r[0].replicas, 9);
        assert_eq!(r[1].replicas, 10);
    }

    #[test]
    fn sort_rows_accepts_leading_dot() {
        let mut a = rows();
        let mut b = rows();
        sort_rows(&mut a, "name").unwrap();
        sort_rows(&mut b, ".name").unwrap();
        assert_eq!(a[0].name, b[0].name);
        assert_eq!(a[0].name, "a");
    }

    #[test]
    fn sort_rows_is_stable_for_equal_keys() {
        let mut r = vec![
            WidgetRow {
                name: "first".into(),
                replicas: 1,
                labels: String::new(),
            },
            WidgetRow {
                name: "second".into(),
                replicas: 1,
                labels: String::new(),
            },
        ];
        sort_rows(&mut r, "replicas").unwrap();
        assert_eq!(r[0].name, "first");
        assert_eq!(r[1].name, "second");
    }

    #[test]
    fn sort_rows_tolerates_unknown_path() {
        let mut r = rows();
        // Should not error or panic; order becomes the null key ordering.
        sort_rows(&mut r, "nonexistent").unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn row_kind_is_derived_and_lowercased() {
        assert_eq!(row_kind::<WidgetRow>(), "widget");
    }

    #[test]
    fn compare_values_prefers_numbers() {
        use std::cmp::Ordering;
        assert_eq!(
            compare_values(&serde_json::json!(9), &serde_json::json!(10)),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&serde_json::json!("a"), &serde_json::json!("b")),
            Ordering::Less
        );
    }

    #[test]
    fn compare_values_orders_mixed_types_by_type_rank() {
        use std::cmp::Ordering;
        // Number before string, regardless of the quote characters that
        // Value::to_string() introduces.
        assert_eq!(
            compare_values(&serde_json::json!(1), &serde_json::json!("a")),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&serde_json::json!(null), &serde_json::json!(0)),
            Ordering::Less
        );
        // Antisymmetric and total: never Equal for unequal values.
        assert_ne!(
            compare_values(&serde_json::json!("a"), &serde_json::json!(1)),
            Ordering::Less
        );
    }
}
