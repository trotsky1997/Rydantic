use crate::{diagnostic, fail, input, Args, Report, Result, KNOWLEDGE};
use semver::{Version, VersionReq};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub fn run(a: &Args) -> Result<Report> {
    let root = a.workspace();
    let manifest = root.join("Cargo.toml");
    let doc = input::toml(&manifest)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|_| fail(4, "RD_INPUT_IO", "Cannot inspect workspace root."))?;
    // Internal observation only; never copy config content into diagnostics.
    let config_present =
        root.join(".cargo/config").exists() || root.join(".cargo/config.toml").exists();
    let mut report = Report::new(
        json!({"verification":"static","project_code_executed":false,"resolution":"unknown","build":"unknown","tests":"unknown","resolved_features":null,"verified_features":null}),
    );
    let mut packages = vec![];
    if doc.get("package").is_some() {
        packages.push((root.clone(), doc.clone()));
    }
    let mut discovery_complete = doc
        .get("workspace")
        .and_then(|w| w.get("exclude"))
        .is_none();
    if let Some(members) = doc.get("workspace").and_then(|w| w.get("members")) {
        let members = members.as_array().ok_or_else(invalid_manifest)?;
        if members.len() > 256 {
            return Err(limit());
        }
        for member in members {
            let member = member.as_str().ok_or_else(invalid_manifest)?;
            // Do not implement Cargo glob/path/exclusion semantics approximately.
            if member.contains(['*', '?', '['])
                || Path::new(member).is_absolute()
                || Path::new(member)
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                discovery_complete = false;
                continue;
            }
            let dir = root.join(member);
            if !dir
                .canonicalize()
                .is_ok_and(|p| p.starts_with(&canonical_root))
            {
                discovery_complete = false;
                continue;
            }
            if dir == root {
                continue;
            }
            if !dir
                .join("Cargo.toml")
                .canonicalize()
                .is_ok_and(|p| p.starts_with(&canonical_root))
            {
                discovery_complete = false;
                continue;
            }
            let d = input::toml(&dir.join("Cargo.toml"))?;
            packages.push((dir, d));
        }
    }
    let selected: Vec<_> = packages
        .iter()
        .filter(|(_, d)| {
            a.get("--package")
                .is_none_or(|n| package_name(d) == Some(n))
        })
        .collect();
    if selected.is_empty() || (a.get("--package").is_some() && selected.len() != 1) {
        return Err(fail(5,"RD_PACKAGE_UNKNOWN","Cannot identify the requested package from explicit manifests. Glob and external workspace members are not expanded."));
    }
    if !discovery_complete {
        report.diagnostics.push(diagnostic("RD_DISCOVERY_INCOMPLETE","warning","discovery","Some workspace members need Cargo resolution; glob and external paths were not expanded."));
    }
    let lock = root.join("Cargo.lock");
    let mut locked = vec![];
    if lock.exists() {
        let l = input::toml(&lock)?;
        let entries = l
            .get("package")
            .and_then(toml::Value::as_array)
            .ok_or_else(invalid_manifest)?;
        for p in entries {
            if p.get("name").and_then(toml::Value::as_str) == Some("rydantic") {
                let version = p
                    .get("version")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(invalid_manifest)?;
                Version::parse(version).map_err(|_| invalid_manifest())?;
                let source = p.get("source").and_then(toml::Value::as_str);
                let kind = match source {
                    None => "path",
                    Some(s) if s.starts_with("registry+") => "registry",
                    Some(s) if s.starts_with("git+") => "git",
                    _ => "unknown",
                };
                locked.push(json!({"version":version,"source_kind":kind,"source_redacted":source.is_some(),"evidence":"lockfile_only"}));
            }
        }
    } else {
        report.diagnostics.push(diagnostic(
            "RD_LOCKFILE_MISSING",
            "warning",
            "discovery",
            "No workspace lockfile was found. Declared versions are not resolved versions.",
        ));
    }
    locked.sort_by_key(|v| v.to_string());
    locked.dedup();
    let knowledge: Value = serde_json::from_str(KNOWLEDGE).expect("valid bundled knowledge");
    let known_features: BTreeSet<_> = knowledge["features"]
        .as_array()
        .expect("feature array")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let mut declarations = vec![];
    for (_, d) in selected {
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            inspect_deps(
                d.get(section),
                section,
                None,
                package_name(d),
                &doc,
                config_present,
                &locked,
                &known_features,
                &mut declarations,
                &mut report,
            )?;
        }
        if let Some(targets) = d.get("target").and_then(toml::Value::as_table) {
            for (target, table) in targets {
                for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    inspect_deps(
                        table.get(section),
                        section,
                        Some(target),
                        package_name(d),
                        &doc,
                        config_present,
                        &locked,
                        &known_features,
                        &mut declarations,
                        &mut report,
                    )?;
                }
            }
        }
    }
    if declarations.is_empty() {
        report.diagnostics.push(diagnostic("RD_DEPENDENCY_NOT_FOUND","warning","discovery","No direct Rydantic declaration was found in the inspected manifests; transitive dependencies are not inferred."));
    }
    report.data["declared_dependencies"] = json!(declarations);
    report.data["locked_dependencies"] = json!(locked);
    report.data["discovery_complete"] = json!(false);
    report.data["discovery_scope"] = json!("explicit_manifests_only");
    report.data["explicit_manifest_reads_complete"] = json!(discovery_complete);
    report.data["lockfile_present"] = json!(lock.exists());
    // Presence only: configuration may contain credentials and is never executed or echoed.
    report.data["cargo_config_present"] =
        json!(root.join(".cargo/config").exists() || root.join(".cargo/config.toml").exists());
    report.data["toolchain_file_present"] =
        json!(root.join("rust-toolchain").exists() || root.join("rust-toolchain.toml").exists());
    report.data["toolchain_installed"] = Value::Null;
    if root.join("rydantic.toml").exists() {
        report.diagnostics.push(diagnostic("RD_CONFIG_UNSUPPORTED","warning","discovery","rydantic.toml is present but this CLI does not load configuration. Use explicit supported flags."));
    }
    if let Some(file) = a.get("--schema") {
        let path = PathBuf::from(file);
        let schema = input::json(&path, 65536)?;
        if !schema.is_object() && !schema.is_boolean() {
            return Err(fail(
                2,
                "RD_SCHEMA_INVALID",
                "Schema annotations require an object or boolean root.",
            ));
        }
        let mut count = 0;
        local_rules(&schema, &mut count);
        report.data["schema"] = json!({"inspection":"annotations_only","local_rules_annotations":count,"valid":null,"portable":null,"runtime_eligible":null,"codegen_eligible":null});
        if count > 0 {
            report.diagnostics.push(diagnostic("RD_LOCAL_RULES_REQUIRED","warning","discovery","Local-rule annotations were found. This is not validation; typed rules require the actual Rust model."));
        }
    }
    Ok(report)
}
fn package_name(d: &toml::Value) -> Option<&str> {
    d.get("package")?.get("name")?.as_str()
}
fn invalid_manifest() -> crate::Failure {
    fail(
        2,
        "RD_MANIFEST_INVALID",
        "Unsupported or malformed manifest/lockfile structure.",
    )
}
fn limit() -> crate::Failure {
    fail(7, "RD_LIMIT_EXCEEDED", "Discovery item limit exceeded.")
}
#[allow(clippy::too_many_arguments)]
fn inspect_deps(
    table: Option<&toml::Value>,
    kind: &str,
    target: Option<&str>,
    package: Option<&str>,
    root: &toml::Value,
    source_config_unknown: bool,
    locked: &[Value],
    known: &BTreeSet<&str>,
    out: &mut Vec<Value>,
    report: &mut Report,
) -> Result<()> {
    let Some(table) = table else { return Ok(()) };
    let table = table.as_table().ok_or_else(invalid_manifest)?;
    for (alias, original) in table {
        let inherited = original.get("workspace").and_then(toml::Value::as_bool) == Some(true);
        let dep = if inherited {
            root.get("workspace")
                .and_then(|w| w.get("dependencies"))
                .and_then(|ds| ds.get(alias))
        } else {
            Some(original)
        };
        let Some(dep) = dep else {
            report.diagnostics.push(diagnostic("RD_INHERITANCE_UNKNOWN","warning","discovery","Workspace dependency inheritance could not be established; aliased Rydantic dependencies may be omitted."));
            continue;
        };
        let actual = dep
            .get("package")
            .and_then(toml::Value::as_str)
            .unwrap_or(alias);
        if actual != "rydantic" {
            continue;
        }
        if out.len() >= 256 {
            return Err(limit());
        }
        let req = dep
            .as_str()
            .or_else(|| dep.get("version").and_then(toml::Value::as_str));
        let requirement = req
            .map(VersionReq::parse)
            .transpose()
            .map_err(|_| invalid_manifest())?;
        let mut features = BTreeSet::new();
        for d in [dep, original] {
            if let Some(fs) = d.get("features") {
                for f in fs.as_array().ok_or_else(invalid_manifest)? {
                    features.insert(f.as_str().ok_or_else(invalid_manifest)?.to_owned());
                }
            }
        }
        let source_kind = if dep.get("git").is_some() {
            "git"
        } else if dep.get("path").is_some() {
            "path"
        } else {
            "registry"
        };
        let candidates: Vec<_> = locked
            .iter()
            .filter(|l| {
                l["source_kind"] == source_kind
                    && requirement.as_ref().is_none_or(|r| {
                        l["version"]
                            .as_str()
                            .and_then(|v| Version::parse(v).ok())
                            .is_some_and(|v| r.matches(&v))
                    })
            })
            .map(|l| l["version"].clone())
            .collect();
        let exact_stable = req.is_some_and(|r| r.trim() == "=0.6.0") && source_kind == "registry";
        let stable_lock =
            candidates.len() == 1 && candidates[0] == "0.6.0" && source_kind == "registry";
        // A lock candidate is never a resolved association: it may be stale or patched.
        let patches = root.get("patch").is_some() || root.get("replace").is_some();
        let can_check =
            exact_stable && !patches && dep.get("registry").is_none() && !source_config_unknown;
        let unavailable: Vec<_> = features
            .iter()
            .filter(|f| can_check && !known.contains(f.as_str()))
            .cloned()
            .collect();
        if !unavailable.is_empty() {
            report.exit = 3;
            let mut d = diagnostic(
                "RD_FEATURE_UNAVAILABLE",
                "error",
                "discovery",
                "The exactly declared 0.6.0 release does not provide a requested feature.",
            );
            d["params"] = json!({"features":unavailable,"core_version":"0.6.0","evidence":"exact_declaration"});
            d["next_action"] = json!({"kind":"review_version_choice"});
            report.diagnostics.push(d);
        }
        if candidates.is_empty() && !locked.is_empty() {
            report.diagnostics.push(diagnostic("RD_LOCK_MISMATCH","warning","discovery","No recorded lock version matches a declaration. Resolution remains unknown; no network or repair was attempted."));
        }
        if !can_check {
            report.diagnostics.push(diagnostic("RD_VERSION_UNKNOWN","warning","discovery","Feature availability is unknown without an exact supported registry declaration; lockfile candidates are not resolved evidence."));
        }
        let base_defaults = dep
            .get("default-features")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let default_features = if inherited {
            base_defaults
                || original
                    .get("default-features")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false)
        } else {
            base_defaults
        };
        out.push(json!({"package":package,"dependency_name":alias,"crate":"rydantic","kind":kind,"target":target,"declared_version":req,"declared_features":features,"declared_default_features":default_features,"optional":original.get("optional").and_then(toml::Value::as_bool).unwrap_or(false),"workspace_inherited":inherited,"source_kind":source_kind,"locked_version_candidates":candidates,"supported_lock_candidate":stable_lock,"version_association":"unknown","feature_knowledge":if can_check{"exact_declaration_0.6.0"}else{"unknown"},"resolved_features":null,"verified_features":null}));
    }
    Ok(())
}
fn local_rules(v: &Value, count: &mut usize) {
    match v {
        Value::Object(o) => {
            if o.contains_key("x-rydantic-local-rules") {
                *count += 1;
            }
            for value in o.values() {
                local_rules(value, count);
            }
        }
        Value::Array(a) => {
            for value in a {
                local_rules(value, count);
            }
        }
        _ => {}
    }
}
