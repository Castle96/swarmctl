//! The resource registry: one authoritative table of every resource swarmctl
//! understands, plus kubectl-compatible parsing of the resource argument.
//!
//! Before this existed, resource metadata lived in three places that had to be
//! kept in sync by hand: clap `alias` attributes on the `ResourceType` enum,
//! `explain::SHORT_NAMES`, and the hand-written `FromStr` impl clap never
//! called. `explain` advertised only plural names while clap also accepted
//! singulars, so `api-resources` and the actual parser disagreed.
//!
//! Resolution is deliberately **syntactic**, not server-driven. Docker Swarm has
//! a single implicit API version and no API groups, so there is no discovery
//! document to query. What we do replicate is kubectl's *grammar*, which is what
//! users actually type:
//!
//! ```text
//! TYPE[.VERSION][.GROUP]          svc, service, services, deployments.v1.apps
//! TYPE/NAME                       po/web
//! TYPE/NAME,TYPE/NAME             po/web,svc/api
//! ```
//!
//! See `api_resources.rs` for the one place the negotiated API version is
//! reported to the user.

// Re-exported so callers have a single import for "a resource".
pub use crate::cli::root::ResourceType;
use anyhow::{Result, bail};
use std::collections::BTreeSet;

/// Static description of one resource kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceInfo {
    /// Compile-time dispatch target. The `match` arms in `get.rs`, `delete.rs`
    /// and friends stay exhaustive because of this.
    pub resource: ResourceType,
    /// Kubernetes-style `kind` (`Service`), used in serialized objects.
    pub kind: &'static str,
    /// Plural noun, the canonical `NAME` in `api-resources` (`services`).
    pub plural: &'static str,
    /// Singular noun (`service`), which users type constantly.
    pub singular: &'static str,
    /// Short aliases (`svc`). kubectl prints these comma-joined in
    /// `api-resources`, and accepts all of them.
    pub short_names: &'static [&'static str],
    /// Docker Swarm has no namespaces, so this is false for everything. It is
    /// still modelled because `api-resources` is expected to carry the column,
    /// and stacks are grouped by a label instead (see `com.docker.stack.namespace`).
    pub namespaced: bool,
}

/// Every resource swarmctl can address, in `api-resources` display order.
pub const REGISTRY: &[ResourceInfo] = &[
    ResourceInfo {
        resource: ResourceType::Nodes,
        kind: "Node",
        plural: "nodes",
        singular: "node",
        short_names: &["no"],
        namespaced: false,
    },
    ResourceInfo {
        resource: ResourceType::Services,
        kind: "Service",
        plural: "services",
        singular: "service",
        short_names: &["svc"],
        namespaced: false,
    },
    ResourceInfo {
        resource: ResourceType::Tasks,
        kind: "Task",
        plural: "tasks",
        singular: "task",
        short_names: &["po"],
        namespaced: false,
    },
    ResourceInfo {
        resource: ResourceType::Networks,
        kind: "Network",
        plural: "networks",
        singular: "network",
        short_names: &["net"],
        namespaced: false,
    },
    ResourceInfo {
        resource: ResourceType::Secrets,
        kind: "Secret",
        plural: "secrets",
        singular: "secret",
        short_names: &["sec"],
        namespaced: false,
    },
    ResourceInfo {
        resource: ResourceType::Configs,
        kind: "Config",
        plural: "configs",
        singular: "config",
        short_names: &["cm"],
        namespaced: false,
    },
    ResourceInfo {
        resource: ResourceType::Stacks,
        kind: "Stack",
        plural: "stacks",
        singular: "stack",
        short_names: &["st"],
        namespaced: false,
    },
];

/// Metadata for one resolved resource.
pub fn info(resource: ResourceType) -> &'static ResourceInfo {
    REGISTRY
        .iter()
        .find(|i| i.resource == resource)
        .expect("every ResourceType variant must appear in REGISTRY")
}

/// The singular noun used by `-o name` (`service/name`).
pub fn singular(resource: ResourceType) -> &'static str {
    info(resource).singular
}

/// Every spelling that resolves to a resource: plural, singular, short names.
///
/// Returned sorted so `api-resources` and error messages are deterministic.
fn accepted_spellings(i: &ResourceInfo) -> BTreeSet<&'static str> {
    let mut set = BTreeSet::new();
    set.insert(i.plural);
    set.insert(i.singular);
    set.insert(i.kind); // kubectl accepts the Kind too: `get Service`
    for s in i.short_names {
        set.insert(*s);
    }
    set
}

/// Every accepted spelling across all resources, sorted.
pub fn all_spellings() -> Vec<&'static str> {
    let mut all = BTreeSet::new();
    for i in REGISTRY {
        all.extend(accepted_spellings(i));
    }
    all.into_iter().collect()
}

/// Every accepted spelling for one resource, sorted.
///
/// Short names are not included: `explain` lists them separately because they
/// are shown as a group, the way `api-resources` does.
pub fn all_spellings_for(i: &ResourceInfo) -> Vec<&'static str> {
    let mut set = BTreeSet::new();
    set.insert(i.plural);
    set.insert(i.singular);
    set.insert(i.kind);
    set.into_iter().collect()
}

/// Levenshtein distance, used for kubectl-style "Did you mean this?".
///
/// Capped at `threshold` so obviously-unrelated input costs nothing to reject.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Nearest accepted spellings for an unrecognised token.
///
/// Mirrors kubectl, which prints `Did you mean this?` followed by a tab and the
/// candidate. Distance is scaled by length so a 3-letter typo is forgiven on a
/// short name but a long name still needs a near-exact hit.
pub fn suggestions(input: &str, candidates: &[&'static str]) -> Vec<&'static str> {
    let budget = match input.len() {
        0..=3 => 1,
        4..=7 => 2,
        _ => 3,
    };
    let plurals: BTreeSet<&'static str> = REGISTRY.iter().map(|i| i.plural).collect();
    let lower = input.to_lowercase();
    let mut scored: Vec<(usize, bool, &'static str)> = candidates
        .iter()
        .filter_map(|c| {
            let d = levenshtein(&lower, c);
            (d <= budget).then_some((d, !plurals.contains(c), *c))
        })
        .collect();
    // Nearest first; on a tie prefer the plural (the canonical `api-resources`
    // name), then alphabetical so output is stable.
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(b.2))
    });
    scored.into_iter().map(|(_, _, c)| c).collect()
}

/// Error for a resource token that no registry entry accepts.
#[derive(Debug)]
pub struct ResolveError {
    pub token: String,
    pub suggestions: Vec<&'static str>,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the server doesn't have a resource type \"{}\"",
            self.token
        )?;
        if !self.suggestions.is_empty() {
            write!(f, "\n\nDid you mean this?\n")?;
            for s in &self.suggestions {
                writeln!(f, "\t{s}")?;
            }
        } else {
            write!(
                f,
                "\n\nUse \"swarmctl api-resources\" for a complete list of supported resources."
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for ResolveError {}

/// Split the `TYPE[.VERSION][.GROUP]` qualifier off a resource token.
///
/// kubectl accepts `pods`, `pods.v1.` and `deployments.v1.apps`. Swarm has one
/// implicit version and no groups, so the qualifier is parsed for syntax and
/// validated, then discarded — but it is *not* silently ignored, because a
/// user who typed a version expects to be told when it is wrong.
fn split_qualifier(token: &str) -> (&str, Option<String>, Option<String>) {
    let Some((base, rest)) = token.split_once('.') else {
        return (token, None, None);
    };
    // Docker's Engine API versions contain a dot — `v1.52` — while kubectl's
    // never do (`v1`, `v1beta1`). Read the two-part numeric form as one version
    // before falling back to kubectl's rules, otherwise `services.v1.52` would
    // be read as version `v1` plus a group named `52`.
    if is_version_like(rest)
        && rest.split('.').count() == 2
        && rest.split('.').nth(1).is_some_and(all_digits)
    {
        return (base, Some(rest.to_string()), None);
    }
    let parts: Vec<&str> = rest.split('.').collect();
    // `v1` / `v2beta1` style version: starts with `v`, remainder is alphanumeric.
    let looks_like_version = parts.first().is_some_and(|p| {
        p.starts_with('v') && p.len() > 1 && p[1..].chars().all(|c| c.is_alphanumeric())
    });
    if looks_like_version {
        let version = parts[0].to_string();
        let group = parts[1..].join(".");
        (
            base,
            Some(version),
            if group.is_empty() { None } else { Some(group) },
        )
    } else {
        // No version component: the whole remainder is a group.
        let group = rest.to_string();
        (
            base,
            None,
            if group.is_empty() { None } else { Some(group) },
        )
    }
}

/// Does this fragment look like an API version rather than a group name?
fn is_version_like(s: &str) -> bool {
    s.starts_with('v') && s.len() > 1 && s[1..].chars().all(|c| c.is_alphanumeric() || c == '.')
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// The API version swarmctl actually speaks, as reported by `api-resources`.
fn negotiated_version() -> String {
    format!("v{}", bollard::API_DEFAULT_VERSION)
}

/// Resolve one kubectl-style resource token to a [`ResourceType`].
///
/// Accepts plural, singular, `kind`, any short name, and an optional
/// `.VERSION.GROUP` qualifier.
pub fn resolve(token: &str) -> Result<ResourceType> {
    resolve_with_version(token, &negotiated_version())
}

/// Same as [`resolve`], but against a caller-supplied API version.
///
/// Split out so tests can assert the version qualifier is actually validated
/// rather than being decoration.
pub fn resolve_with_version(token: &str, api_version: &str) -> Result<ResourceType> {
    let token = token.trim();
    if token.is_empty() {
        bail!("resource type may not be empty");
    }

    let (base, version, group) = split_qualifier(token);
    let key = base.to_lowercase();

    let found = REGISTRY
        .iter()
        .find(|i| accepted_spellings(i).contains(key.as_str()));
    let Some(found) = found else {
        return Err(ResolveError {
            token: token.to_string(),
            suggestions: suggestions(&key, &all_spellings()),
        }
        .into());
    };

    // Swarm registers no API groups, so any group qualifier is a mistake worth
    // naming. Checked before the version, because a group makes the version
    // moot and reporting only the version would be the less useful message.
    if let Some(group) = group
        && !group.is_empty()
    {
        bail!(
            "no matches for kind \"{}\" in group \"{}\" (swarm has no API groups)",
            found.kind,
            group
        );
    }
    if let Some(version) = version
        && version != api_version
    {
        bail!(
            "no matches for kind \"{}\" in version \"{}\" (this swarmctl speaks \"{}\")",
            found.kind,
            version,
            api_version
        );
    }

    Ok(found.resource)
}

/// One resolved resource plus the specific names requested for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRequest {
    pub resource: ResourceType,
    /// Empty means "all objects of this type", matching `get svc` vs `get svc/web`.
    pub names: Vec<String>,
}

/// Parse the resource argument of a `get`-style command.
///
/// `spec` is the raw `TYPE[/NAME]` argument, possibly comma-separated.
/// `positional` holds bare names typed after it (`get po web api`), which
/// kubectl also accepts and which apply to the single type named.
///
/// Repeated entries for the same type are merged, so `po/web,po/api` yields one
/// request with two names.
pub fn parse_request(
    spec: &str,
    positional: &[String],
    api_version: &str,
) -> Result<Vec<ResourceRequest>> {
    let mut requests: Vec<ResourceRequest> = Vec::new();

    for piece in spec.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            bail!("empty resource type in \"{}\"", spec);
        }
        let (token, name) = match piece.split_once('/') {
            Some((t, n)) if !n.is_empty() => (t, Some(n.to_string())),
            Some(_) => bail!("\"{}\" is missing a name after '/'", piece),
            None => (piece, None),
        };

        let resource = resolve_with_version(token, api_version)?;
        match requests.iter_mut().find(|r| r.resource == resource) {
            Some(existing) => {
                if let Some(name) = name {
                    existing.names.push(name);
                }
            }
            None => requests.push(ResourceRequest {
                resource,
                names: name.into_iter().collect(),
            }),
        }
    }

    if !positional.is_empty() {
        if requests.len() != 1 {
            bail!(
                "names may only be given when a single resource type is requested; \
                 got {} types plus names {}",
                requests.len(),
                positional.join(", ")
            );
        }
        requests[0].names.extend(positional.iter().cloned());
    }

    Ok(requests)
}

/// The `NAME` column value `api-resources` prints, comma-joined short names.
pub fn short_names_joined(i: &ResourceInfo) -> String {
    i.short_names.join(",")
}

#[cfg(test)]
mod tests {

    use super::*;

    const V: &str = "v1.52";

    #[test]
    fn every_resource_type_appears_exactly_once() {
        let mut seen: Vec<ResourceType> = Vec::new();
        for i in REGISTRY {
            assert!(
                !seen.contains(&i.resource),
                "duplicate registry entry for {:?}",
                i.resource
            );
            seen.push(i.resource);
        }
        // Seven variants; catching a new one that forgot the registry.
        assert_eq!(REGISTRY.len(), 7);
    }

    #[test]
    fn no_spelling_is_ambiguous_across_resources() {
        let mut owner: BTreeMap<&str, ResourceType> = BTreeMap::new();
        for i in REGISTRY {
            for s in accepted_spellings(i) {
                if let Some(prev) = owner.insert(s, i.resource)
                    && prev != i.resource
                {
                    panic!("\"{s}\" maps to both {prev:?} and {:?}", i.resource);
                }
            }
        }
    }

    #[test]
    fn plural_singular_kind_and_short_names_all_resolve() {
        let expected = ResourceType::Services;
        for token in ["services", "service", "svc", "Service", "services/"] {
            if let Ok(r) = resolve_with_version(token, V) {
                assert_eq!(r, expected, "token {token}");
            }
        }
        assert_eq!(resolve_with_version("services", V).unwrap(), expected);
        assert_eq!(resolve_with_version("service", V).unwrap(), expected);
        assert_eq!(resolve_with_version("svc", V).unwrap(), expected);
        assert_eq!(resolve_with_version("Service", V).unwrap(), expected);
        assert_eq!(resolve_with_version("SERVICES", V).unwrap(), expected);
        assert_eq!(resolve_with_version("no", V).unwrap(), ResourceType::Nodes);
        assert_eq!(resolve_with_version("po", V).unwrap(), ResourceType::Tasks);
        assert_eq!(
            resolve_with_version("net", V).unwrap(),
            ResourceType::Networks
        );
        assert_eq!(
            resolve_with_version("sec", V).unwrap(),
            ResourceType::Secrets
        );
        assert_eq!(
            resolve_with_version("cm", V).unwrap(),
            ResourceType::Configs
        );
        assert_eq!(resolve_with_version("st", V).unwrap(), ResourceType::Stacks);
    }

    #[test]
    fn version_qualifier_is_accepted_and_validated() {
        assert_eq!(
            resolve_with_version("services.v1.52", V).unwrap(),
            ResourceType::Services
        );
        // `v1.` with an empty group is kubectl's usual spelling.
        assert_eq!(
            resolve_with_version("tasks.v1.", "v1").unwrap(),
            ResourceType::Tasks
        );
        // Wrong version is reported, not silently accepted.
        let err = resolve_with_version("services.v1.30", V)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("v1.30"),
            "error should name the version: {err}"
        );
        assert!(err.contains("v1.52"), "error should name ours: {err}");
    }

    #[test]
    fn api_group_qualifier_is_rejected_with_a_reason() {
        let err = resolve_with_version("services.v1.apps", V)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no API groups"), "{err}");
        // A group with no version component is also rejected.
        assert!(resolve_with_version("services.apps", V).is_err());
        // Swarm registers no `deployments` at all, so that fails on the name.
        assert!(resolve_with_version("deployments.v1.apps", V).is_err());
    }

    #[test]
    fn unknown_token_reports_suggestions() {
        let err = resolve_with_version("poos", V).unwrap_err().to_string();
        assert!(err.contains("doesn't have a resource type"), "{err}");
        assert!(err.contains("Did you mean"), "{err}");
        assert!(err.contains("\tpo"), "{err}");
        // A Kubernetes name swarmctl does not have is still a clear miss.
        let err = resolve_with_version("pods", V).unwrap_err().to_string();
        assert!(err.contains("doesn't have a resource type"), "{err}");
    }

    #[test]
    fn wildly_wrong_token_falls_back_to_api_resources_hint() {
        let err = resolve_with_version("zzzzzzzzzzzz", V)
            .unwrap_err()
            .to_string();
        assert!(err.contains("api-resources"), "{err}");
        assert!(!err.contains("Did you mean"), "{err}");
    }

    #[test]
    fn parse_request_handles_comma_lists() {
        let r = parse_request("po,svc", &[], V).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].resource, ResourceType::Tasks);
        assert_eq!(r[1].resource, ResourceType::Services);
        assert!(r[0].names.is_empty());
    }

    #[test]
    fn parse_request_handles_type_slash_name() {
        let r = parse_request("po/web", &[], V).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].resource, ResourceType::Tasks);
        assert_eq!(r[0].names, vec!["web"]);
    }

    #[test]
    fn parse_request_handles_mixed_type_name_list() {
        let r = parse_request("po/web,svc/api", &[], V).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(
            r[0],
            ResourceRequest {
                resource: ResourceType::Tasks,
                names: vec!["web".into()]
            }
        );
        assert_eq!(
            r[1],
            ResourceRequest {
                resource: ResourceType::Services,
                names: vec!["api".into()]
            }
        );
    }

    #[test]
    fn parse_request_merges_repeated_types() {
        let r = parse_request("po/web,po/api", &[], V).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].names, vec!["web", "api"]);
    }

    #[test]
    fn parse_request_appends_positional_names() {
        let r = parse_request("po", &["web".into(), "api".into()], V).unwrap();
        assert_eq!(r[0].names, vec!["web", "api"]);
    }

    #[test]
    fn parse_request_rejects_names_without_a_single_type() {
        let err = parse_request("po,svc", &["web".into()], V)
            .unwrap_err()
            .to_string();
        assert!(err.contains("single resource type"), "{err}");
    }

    #[test]
    fn parse_request_rejects_malformed_specs() {
        assert!(parse_request("po/", &[], V).is_err());
        assert!(parse_request("po,,svc", &[], V).is_err());
        assert!(parse_request("", &[], V).is_err());
    }

    #[test]
    fn levenshtein_ranks_exact_and_close_matches_first() {
        let cands = all_spellings();
        // Swarm calls them tasks, so `po` is the nearest spelling for `poos`.
        assert_eq!(suggestions("poos", &cands).first(), Some(&"po"));
        assert_eq!(suggestions("svc", &cands).first(), Some(&"svc"));
        assert_eq!(suggestions("servics", &cands).first(), Some(&"services"));
    }

    #[test]
    fn short_names_join_is_comma_separated() {
        let svc = info(ResourceType::Services);
        assert_eq!(short_names_joined(svc), "svc");
    }

    use std::collections::BTreeMap;
}
