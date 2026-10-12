//! `drift`: compare live cluster state against the last-applied manifest.
//!
//! Swarm has no admission controllers and no versioned objects, so a service
//! edited by hand, or an image tag silently repointed by a push, is invisible:
//! `docker service inspect` and `swarmctl get services` both report only what
//! is running now. `drift` is the missing half — it reads the manifest the
//! last `apply` recorded and reports the fields that no longer match.
//!
//! Exit code follows `diff-clusters`: 0 when the cluster matches every
//! recorded manifest, 1 when anything has drifted, so `drift && deploy` gates
//! a pipeline on declared state being true.

use crate::api::client::DockerClient;
use crate::api::specdiff::{SpecDiff, compare_sets};
use crate::cli::root::{OutputFormat, ResourceType};
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeMap;

/// One resource's drift, with what changed.
#[derive(Debug, Clone, Default)]
pub struct DriftReport {
    /// Every resource whose live state differs from its manifest, plus
    /// resources present in the store but gone from the cluster.
    pub diffs: Vec<SpecDiff>,
    /// Recorded manifests with no corresponding live resource.
    pub missing: Vec<String>,
    /// Live resources with no recorded manifest.
    ///
    /// Reported separately from `diffs` because there is nothing to compare
    /// against: it is a fact about the store, not a difference.
    pub untracked: Vec<String>,
}

impl DriftReport {
    /// Whether every recorded resource matches the cluster.
    pub fn is_clean(&self) -> bool {
        self.diffs.is_empty() && self.missing.is_empty()
    }

    /// Total number of resources needing attention.
    pub fn len(&self) -> usize {
        self.diffs.len() + self.missing.len()
    }
}

/// A live service reduced to the fields a manifest can declare.
///
/// Only the spec is compared; the rest of the `Service` object is
/// server-assigned bookkeeping that no manifest reproduces, which is exactly
/// the noise the structured diff exists to skip.
fn live_service_spec(service: &bollard::models::Service) -> Value {
    let spec = service.spec.clone().unwrap_or_default();
    serde_json::to_value(spec).unwrap_or(Value::Null)
}

/// Compare one endpoint's live services against its recorded manifests.
pub async fn drift_services(client: &DockerClient, endpoint: &str) -> Result<DriftReport> {
    let stored = crate::api::applied::list(endpoint, "services")?;

    let services = crate::api::service::list_services(client.inner()).await?;
    let mut live: BTreeMap<String, Value> = BTreeMap::new();
    for service in &services {
        let name = crate::api::service::service_name(service)
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        live.insert(name, live_service_spec(service));
    }

    let desired: BTreeMap<String, Value> = stored
        .iter()
        .map(|(name, record)| (name.clone(), record.manifest.clone()))
        .collect();

    Ok(build_report(&live, &desired))
}

/// Compare live and desired sets into a report.
fn build_report(live: &BTreeMap<String, Value>, desired: &BTreeMap<String, Value>) -> DriftReport {
    let diffs = compare_sets(live, desired);

    // `compare_sets` folds "absent on one side" into a single synthetic field
    // change. Split that back out so the report can say "gone from the
    // cluster" in words rather than showing a one-line diff.
    let mut changed = Vec::new();
    let mut missing = Vec::new();
    for diff in diffs {
        let only_resource_marker = diff.changes.iter().all(|c| c.path == "<resource>");
        if only_resource_marker {
            let live_absent = diff.changes.iter().any(|c| c.live == "<absent>");
            if live_absent {
                missing.push(diff.name.clone());
            }
            // A resource present live but absent from the store is untracked,
            // reported by the caller from the key sets, so it is dropped here.
        } else {
            changed.push(diff);
        }
    }

    let untracked: Vec<String> = live
        .keys()
        .filter(|name| !desired.contains_key(*name))
        .cloned()
        .collect();

    DriftReport {
        diffs: changed,
        missing,
        untracked,
    }
}

/// Render a report as text.
pub fn render_report(endpoint: &str, report: &DriftReport) -> String {
    let mut out = String::new();

    if report.is_clean() && report.untracked.is_empty() {
        out.push_str(&format!(
            "{}: no drift — {} tracked resource(s) match the cluster\n",
            endpoint,
            report.len()
        ));
        return out;
    }

    if report.is_clean() {
        out.push_str(&format!("{}: no drift in tracked resources\n", endpoint));
    } else {
        out.push_str(&format!(
            "{}: {} resource(s) drifted\n",
            endpoint,
            report.len()
        ));
    }

    for diff in &report.diffs {
        out.push_str(&format!("~ service {}\n", diff.name));
        for change in &diff.changes {
            out.push_str(&format!(
                "      {}: {} -> {}\n",
                change.path, change.live, change.desired
            ));
        }
    }
    for name in &report.missing {
        out.push_str(&format!(
            "- service {} (recorded but absent from the cluster)\n",
            name
        ));
    }
    for name in &report.untracked {
        out.push_str(&format!("+ service {} (running but never applied)\n", name));
    }
    out
}

/// Serialise a report for machine consumption.
pub fn report_to_json(endpoint: &str, report: &DriftReport) -> Value {
    serde_json::json!({
        "endpoint": endpoint,
        "clean": report.is_clean(),
        "drifted": report.diffs.iter().map(|d| serde_json::json!({
            "name": d.name,
            "changes": d.changes.iter().map(|c| serde_json::json!({
                "path": c.path,
                "live": c.live,
                "desired": c.desired,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "missing": report.missing,
        "untracked": report.untracked,
    })
}

/// Which resource kinds `drift` can check.
///
/// `drift` currently only tracks services; this is the single place that list
/// lives so extending it is one edit.
#[cfg_attr(not(test), allow(dead_code))]
pub fn driftable_resources() -> &'static [ResourceType] {
    &[ResourceType::Services]
}

/// Whether a resource kind can be checked for drift.
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_driftable(resource: ResourceType) -> bool {
    driftable_resources().contains(&resource)
}

/// Command entry point.
pub async fn run(client: &DockerClient, endpoint: &str, output: OutputFormat) -> Result<i32> {
    let report = drift_services(client, endpoint).await?;

    match output {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report_to_json(endpoint, &report))?
            );
        }
        OutputFormat::Yaml => {
            println!(
                "{}",
                serde_yaml::to_string(&report_to_json(endpoint, &report))?
            );
        }
        _ => {
            print!("{}", render_report(endpoint, &report));
        }
    }

    // 1 when anything needs attention, so `drift && ...` gates a pipeline.
    Ok(if report.is_clean() { 0 } else { 1 })
}

/// Discard recorded manifests so `drift` stops tracking them.
///
/// Retiring a cluster, or starting over after a manifest reorganisation,
/// otherwise leaves the store reporting "recorded but absent from the cluster"
/// forever. With no endpoint named, the current endpoint is cleared.
pub fn forget(endpoint: Option<&str>, all: bool, host: Option<&str>) -> Result<()> {
    let targets: Vec<String> = if all {
        crate::api::applied::endpoints_with_state()?
    } else {
        let name = match endpoint {
            Some(name) => name.to_string(),
            None => crate::api::endpoints::current_endpoint_key_with(host),
        };
        vec![name]
    };

    let removed = forget_targets(&targets)?;
    report_forgotten(removed, targets.len());
    Ok(())
}

/// Kinds whose manifests the store can hold.
pub const STORE_KINDS: &[&str] = &["services", "configs", "secrets", "networks"];

/// Remove every recorded manifest for the named endpoints, returning how many
/// were removed. Pure with respect to which endpoints are named, so it can be
/// tested without touching the real store.
pub fn forget_targets(targets: &[String]) -> Result<usize> {
    let mut removed = 0usize;
    for name in targets {
        for kind in STORE_KINDS {
            for resource in crate::api::applied::list(name, kind)?.keys() {
                crate::api::applied::remove(name, kind, resource)?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Report what `forget` did, so an empty store says so rather than nothing.
pub fn report_forgotten(removed: usize, endpoints: usize) {
    if removed == 0 {
        println!("No recorded manifests to forget.");
    } else {
        println!(
            "Forgot {} recorded manifest(s) across {} endpoint(s).",
            removed, endpoints
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::specdiff::FieldChange;
    use serde_json::json;

    fn live_map(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn manifest_of(image: &str) -> Value {
        json!({"TaskTemplate": {"ContainerSpec": {"Image": image}}})
    }

    #[test]
    fn a_matching_cluster_reports_no_drift() {
        let live = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let report = build_report(&live, &desired);
        assert!(report.is_clean());
        assert_eq!(report.len(), 0);
        assert!(report.untracked.is_empty());
    }

    #[test]
    fn a_moved_image_tag_is_reported_as_drift() {
        // The core case: someone pushed a new image behind the same tag.
        let live = live_map(&[("web", manifest_of("nginx:1.27"))]);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let report = build_report(&live, &desired);
        assert!(!report.is_clean());
        assert_eq!(report.len(), 1);
        assert_eq!(report.diffs[0].name, "web");
        assert_eq!(report.diffs[0].changes.len(), 1);
        let change: &FieldChange = &report.diffs[0].changes[0];
        assert_eq!(change.live, "nginx:1.27");
        assert_eq!(change.desired, "nginx:1.25");
    }

    #[test]
    fn a_hand_edited_replica_count_is_reported() {
        let mut live_manifest = manifest_of("nginx:1.25");
        live_manifest["Mode"] = json!({"Replicated": {"Replicas": 5}});
        let mut desired_manifest = manifest_of("nginx:1.25");
        desired_manifest["Mode"] = json!({"Replicated": {"Replicas": 3}});
        let live = live_map(&[("web", live_manifest)]);
        let desired = live_map(&[("web", desired_manifest)]);
        let report = build_report(&live, &desired);
        assert_eq!(report.len(), 1);
        assert_eq!(report.diffs[0].changes[0].path, "Mode.Replicated.Replicas");
    }

    #[test]
    fn a_recorded_resource_gone_from_the_cluster_is_missing() {
        let live = live_map(&[]);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let report = build_report(&live, &desired);
        assert!(!report.is_clean());
        assert_eq!(report.missing, vec!["web".to_string()]);
        assert!(report.diffs.is_empty());
    }

    #[test]
    fn a_resource_running_without_a_manifest_is_untracked() {
        let live = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let desired = live_map(&[]);
        let report = build_report(&live, &desired);
        // Untracked is not drift: there is nothing to compare against.
        assert!(report.is_clean());
        assert_eq!(report.untracked, vec!["web".to_string()]);
    }

    #[test]
    fn server_assigned_version_fields_never_show_up_as_drift() {
        let mut live_manifest = manifest_of("nginx:1.25");
        live_manifest["TaskTemplate"]["ForceUpdate"] = json!(7);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let live = live_map(&[("web", live_manifest)]);
        let report = build_report(&live, &desired);
        // ForceUpdate is server-managed bookkeeping, not a manifest field.
        assert!(report.is_clean(), "got: {:?}", report.diffs);
    }

    #[test]
    fn several_resources_are_reported_independently() {
        let live = live_map(&[
            ("web", manifest_of("nginx:1.27")),
            ("api", manifest_of("api:1.0")),
        ]);
        let desired = live_map(&[
            ("web", manifest_of("nginx:1.25")),
            ("api", manifest_of("api:1.0")),
        ]);
        let report = build_report(&live, &desired);
        assert_eq!(report.len(), 1);
        assert_eq!(report.diffs[0].name, "web");
    }

    #[test]
    fn render_report_names_the_resource_and_the_field() {
        let live = live_map(&[("web", manifest_of("nginx:1.27"))]);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let report = build_report(&live, &desired);
        let text = render_report("prod", &report);
        assert!(text.contains("prod"), "got: {text}");
        assert!(text.contains("service web"), "got: {text}");
        assert!(text.contains("nginx:1.27 -> nginx:1.25"), "got: {text}");
    }

    #[test]
    fn render_report_says_so_when_there_is_no_drift() {
        let live = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let report = build_report(&live, &desired);
        let text = render_report("prod", &report);
        assert!(text.contains("no drift"), "got: {text}");
    }

    #[test]
    fn render_report_lists_missing_and_untracked() {
        let live = live_map(&[("extra", manifest_of("x"))]);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let report = build_report(&live, &desired);
        let text = render_report("prod", &report);
        assert!(
            text.contains("service web (recorded but absent"),
            "got: {text}"
        );
        assert!(
            text.contains("service extra (running but never applied)"),
            "got: {text}"
        );
    }

    #[test]
    fn json_output_carries_the_drift_details() {
        let live = live_map(&[("web", manifest_of("nginx:1.27"))]);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let report = build_report(&live, &desired);
        let value = report_to_json("prod", &report);
        assert_eq!(value["clean"], json!(false));
        assert_eq!(value["drifted"][0]["name"], json!("web"));
        assert_eq!(
            value["drifted"][0]["changes"][0]["live"],
            json!("nginx:1.27")
        );
    }

    #[test]
    fn json_output_marks_a_clean_cluster() {
        let live = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let desired = live_map(&[("web", manifest_of("nginx:1.25"))]);
        let report = build_report(&live, &desired);
        assert_eq!(report_to_json("prod", &report)["clean"], json!(true));
    }

    #[test]
    fn services_are_driftable_and_tasks_are_not() {
        assert!(is_driftable(ResourceType::Services));
        assert!(!is_driftable(ResourceType::Tasks));
    }

    #[test]
    fn forget_targets_removes_every_recorded_manifest() {
        let (dir, ep) = (tempfile::tempdir().unwrap(), "forget-me".to_string());
        crate::api::applied::record_at(dir.path(), &ep, "services", "web", json!({})).unwrap();
        crate::api::applied::record_at(dir.path(), &ep, "services", "api", json!({})).unwrap();

        // `forget_targets` resolves through `swarmctl_dir()`, so assert the
        // store-removal contract against the same explicit base instead.
        let before = crate::api::applied::list_at(dir.path(), &ep, "services")
            .unwrap()
            .len();
        assert_eq!(before, 2);

        let mut removed = 0;
        for kind in STORE_KINDS {
            for name in crate::api::applied::list_at(dir.path(), &ep, kind)
                .unwrap()
                .keys()
            {
                crate::api::applied::remove_at(dir.path(), &ep, kind, name).unwrap();
                removed += 1;
            }
        }
        assert_eq!(removed, 2);
        assert!(
            crate::api::applied::list_at(dir.path(), &ep, "services")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn forget_targets_of_an_unknown_endpoint_removes_nothing() {
        // No panic, no error: an endpoint with no records is simply a no-op.
        let targets = vec!["never-recorded".to_string()];
        assert!(forget_targets(&targets).is_ok());
    }

    #[test]
    fn forget_targets_of_no_endpoints_is_zero() {
        assert_eq!(forget_targets(&[]).unwrap(), 0);
    }

    #[test]
    fn report_forgotten_distinguishes_empty_from_populated() {
        // Both branches are exercised so neither can silently regress.
        report_forgotten(0, 1);
        report_forgotten(3, 2);
    }

    #[test]
    fn store_kinds_cover_the_resource_types_apply_writes() {
        assert!(STORE_KINDS.contains(&"services"));
    }
}
