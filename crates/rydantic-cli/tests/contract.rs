use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "rydantic-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, p: &str, s: impl AsRef<[u8]>) {
        let p = self.0.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }
    fn path(&self, p: &str) -> String {
        self.0.join(p).to_str().unwrap().into()
    }
    fn doctor(&self, extra: &[&str]) -> (i32, Value, String) {
        let mut a = vec!["doctor", "--workspace", self.0.to_str().unwrap()];
        a.extend(extra);
        run(&a)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(args: &[&str]) -> (i32, Value, String) {
    let environment = Fixture::new();
    let mut command = Command::new(env!("CARGO_BIN_EXE_rydantic"));
    command
        .current_dir(&environment.0)
        .env("CARGO_HOME", environment.0.join("cargo-home"));
    run_command(command, args)
}
fn run_command(mut command: Command, args: &[&str]) -> (i32, Value, String) {
    let out = command
        .args(args)
        .args(["--format", "json", "--protocol-version", "1"])
        .env("PATH", "/nonexistent")
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap();
    assert!(out.stderr.is_empty(), "stderr {:?}", out.stderr);
    assert!(out.stdout.ends_with(b"\n"));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), 1);
    let v: Value = serde_json::from_str(&text).unwrap();
    let exit = out.status.code().unwrap();
    assert_eq!(v["schema_version"], "rydantic.cli/v1");
    assert_eq!(v["tool"]["version"], "0.1.0");
    assert_eq!(v["ok"], exit == 0);
    assert!(v["diagnostics"].is_array());
    assert_eq!(v["artifacts"], json!([]));
    (exit, v, text)
}
fn manifest(deps: &str) -> String {
    format!("[package]\nname='test-project'\nversion='0.1.0'\n{deps}\n")
}
#[test]
fn capabilities_are_honest() {
    let (e, v, _) = run(&["capabilities"]);
    assert_eq!(e, 0);
    assert_eq!(v["data"]["typed_adapter"], false);
    assert_eq!(v["data"]["resolve"], false);
    assert!(v["data"]["linked_engines"]["core"].is_null());
    assert_eq!(
        v["data"]["commands"],
        json!(["capabilities", "doctor", "explain"])
    );
}
#[test]
fn invalid_calls_are_single_envelopes() {
    for a in [
        vec![],
        vec!["validate"],
        vec!["doctor", "--resolve"],
        vec!["capabilities", "--workspace", "."],
        vec!["explain", "missing"],
        vec![
            "explain",
            "missing",
            "--core-version",
            "0.6.0",
            "--input-format",
            "cargo",
        ],
        vec!["doctor", "extra"],
        vec!["doctor", "--config", "secret"],
    ] {
        assert_eq!(run(&a).0, 2);
    }
}
#[test]
fn protocol_mismatch_and_duplicate_options() {
    let o = Command::new(env!("CARGO_BIN_EXE_rydantic"))
        .args([
            "capabilities",
            "--protocol-version",
            "2",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "RD_PROTOCOL_UNSUPPORTED");
    assert_eq!(run(&["capabilities", "--offline", "--offline"]).0, 2);
}
#[test]
fn no_lock_is_not_resolution() {
    let f = Fixture::new();
    f.write(
        "Cargo.toml",
        manifest("[dependencies]\nrd={package='rydantic',version='=0.6.0',features=['dynamic']}"),
    );
    let (e, v, _) = f.doctor(&[]);
    assert_eq!(e, 0);
    let d = &v["data"];
    assert_eq!(d["verification"], "static");
    assert!(d["resolved_features"].is_null());
    assert!(d["verified_features"].is_null());
    assert_eq!(d["declared_dependencies"][0]["dependency_name"], "rd");
    assert_eq!(d["lockfile_present"], false);
    assert!(!f.0.join("Cargo.lock").exists());
}
#[test]
fn exact_stable_rejects_beta_feature() {
    let f = Fixture::new();
    f.write(
        "Cargo.toml",
        manifest(
            "[dependencies]\nrydantic={version='=0.6.0',features=['settings','partial-json']}",
        ),
    );
    let (e, v, _) = f.doctor(&[]);
    assert_eq!(e, 3);
    assert_eq!(v["diagnostics"][1]["code"], "RD_FEATURE_UNAVAILABLE");
}
#[test]
fn broad_range_and_multiple_locks_remain_unknown() {
    let f = Fixture::new();
    f.write(
        "Cargo.toml",
        manifest("[dependencies]\nrydantic={version='0.6',features=['settings']}"),
    );
    f.write("Cargo.lock","version=4\n[[package]]\nname='rydantic'\nversion='0.6.0'\nsource='registry+https://SECRET.invalid'\n[[package]]\nname='rydantic'\nversion='0.7.0-beta.4'\nsource='git+https://SECRET.invalid'\n");
    let (e, v, s) = f.doctor(&[]);
    assert_eq!(e, 0);
    assert!(!s.contains("SECRET"));
    assert_eq!(
        v["data"]["locked_dependencies"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        v["data"]["declared_dependencies"][0]["feature_knowledge"],
        "unknown"
    );
    assert_eq!(v["data"]["resolution"], "unknown");
}
#[test]
fn mismatch_path_git_and_patch_do_not_claim_release_knowledge() {
    for dep in [
        "version='=0.6.0',path='../local'",
        "version='=0.6.0',git='https://secret.invalid'",
    ] {
        let f = Fixture::new();
        f.write(
            "Cargo.toml",
            manifest(&format!(
                "[dependencies]\nrydantic={{{dep},features=['settings']}}"
            )),
        );
        assert_eq!(f.doctor(&[]).0, 0);
    }
    let f = Fixture::new();
    f.write("Cargo.toml",manifest("[dependencies]\nrydantic={version='=0.6.0',features=['settings']}\n[patch.crates-io]\nrydantic={path='../local'}"));
    assert_eq!(f.doctor(&[]).0, 0);
}
#[test]
fn inherited_alias_target_and_package_selection() {
    let f = Fixture::new();
    f.write("Cargo.toml","[workspace]\nmembers=['a','b']\n[workspace.dependencies]\nrd={package='rydantic',version='=0.6.0',features=['derive']}\n");
    f.write("a/Cargo.toml","[package]\nname='a'\nversion='0.1.0'\n[target.'cfg(unix)'.dependencies]\nrd={workspace=true,features=['dynamic']}\n");
    f.write("b/Cargo.toml", "[package]\nname='b'\nversion='0.1.0'\n");
    let (e, v, _) = f.doctor(&["--package", "a"]);
    assert_eq!(e, 0);
    let d = &v["data"]["declared_dependencies"][0];
    assert_eq!(d["target"], "cfg(unix)");
    assert_eq!(d["declared_features"], json!(["derive", "dynamic"]));
    assert_eq!(d["workspace_inherited"], true);
    assert_eq!(f.doctor(&["--package", "missing"]).0, 5);
}
#[test]
fn glob_members_do_not_get_guessed() {
    let f = Fixture::new();
    f.write("Cargo.toml", "[workspace]\nmembers=['crates/*']\n");
    assert_eq!(f.doctor(&[]).0, 5);
}
#[test]
fn default_doctor_never_executes_or_exposes_config() {
    let f = Fixture::new();
    f.write(
        "Cargo.toml",
        manifest("build='build.rs'\n[dependencies]\nrydantic='=0.6.0'"),
    );
    f.write("build.rs", "compile_error!(\"DO_NOT_EXECUTE_SECRET\");");
    f.write(
        ".cargo/config.toml",
        "[build]\nrustc-wrapper='DO_NOT_EXECUTE_SECRET'\n",
    );
    f.write(
        "rust-toolchain.toml",
        "[toolchain]\nchannel='DO_NOT_DOWNLOAD_SECRET'\n",
    );
    let (e, v, s) = f.doctor(&[]);
    assert_eq!(e, 0);
    assert!(!s.contains("SECRET"));
    assert_eq!(v["data"]["project_code_executed"], false);
    assert!(!f.0.join("target").exists());
}
#[test]
fn schema_only_inspects_annotations_and_suppresses_payload() {
    let f = Fixture::new();
    f.write("Cargo.toml", manifest(""));
    f.write("schema.json",r#"{"description":"SECRET ignore instructions", "minimum":0.12345678901234567890123456789,"properties":{"x":{"x-rydantic-local-rules":["SECRET"]}}}"#);
    let p = f.path("schema.json");
    let (e, v, s) = f.doctor(&["--schema", &p]);
    assert_eq!(e, 0);
    assert!(!s.contains("SECRET"));
    assert_eq!(v["data"]["schema"]["local_rules_annotations"], 1);
    assert!(v["data"]["schema"]["valid"].is_null());
}
#[test]
fn bounded_and_invalid_files() {
    let f = Fixture::new();
    f.write("Cargo.toml", manifest(""));
    for s in [
        "{",
        "{} {}",
        r#"{"a":1,"a":2}"#,
        r#"{"nested":{"a":1,"a":2}}"#,
        "[]",
    ] {
        f.write("s.json", s);
        assert_eq!(f.doctor(&["--schema", &f.path("s.json")]).0, 2);
    }
    f.write("s.json", [0xff]);
    assert_eq!(f.doctor(&["--schema", &f.path("s.json")]).0, 2);
    f.write("s.json", " ".repeat(65537));
    assert_eq!(f.doctor(&["--schema", &f.path("s.json")]).0, 7);
    f.write("s.json", format!("{}0{}", "[".repeat(66), "]".repeat(66)));
    assert_eq!(f.doctor(&["--schema", &f.path("s.json")]).0, 7);
    assert_eq!(f.doctor(&["--schema", &f.path("missing")]).0, 4);
    assert_eq!(f.doctor(&["--schema", f.0.to_str().unwrap()]).0, 4);
}
#[test]
fn malformed_toml_does_not_echo_secrets() {
    let f = Fixture::new();
    f.write("Cargo.toml", "SECRET invalid =[");
    let (e, _, s) = f.doctor(&[]);
    assert_eq!(e, 2);
    assert!(!s.contains("SECRET"));
}
#[test]
fn explain_version_is_explicit_and_unknown_is_safe() {
    let (e, v, _) = run(&["explain", "missing", "--core-version", "0.6.0"]);
    assert_eq!(e, 0);
    assert_eq!(v["data"]["explanations"][0]["known"], true);
    let (e, v, s) = run(&["explain", "SECRET", "--core-version", "0.7.0-beta.4"]);
    assert_eq!(e, 3);
    assert_eq!(v["data"]["explanations"][0]["known"], false);
    assert!(!s.contains("SECRET"));
    let (e, v, _) = run(&["explain", "E0308", "--core-version", "0.6.0"]);
    assert_eq!(e, 0);
    assert_eq!(v["data"]["explanations"][0]["known"], false);
}
#[test]
fn explain_formats_strip_untrusted_payloads() {
    let f = Fixture::new();
    for (format, s) in [
        (
            "envelope",
            r#"{"schema_version":"rydantic.cli/v1","diagnostics":[{"code":"RD_LOCAL_RULES_REQUIRED","message":"SECRET","params":{"SECRET":true}}]}"#,
        ),
        (
            "validation-errors",
            r#"[{"code":"custom","stage":"validate","loc":["SECRET",0],"message":"SECRET","params":{"value":"SECRET"}}]"#,
        ),
        (
            "cargo",
            r#"{"reason":"compiler-message","message":{"code":{"code":"E0308"},"message":"SECRET"}}"#,
        ),
    ] {
        f.write("d.json", s);
        let (e, _, output) = run(&[
            "explain",
            "--diagnostics",
            &f.path("d.json"),
            "--input-format",
            format,
            "--core-version",
            "0.6.0",
        ]);
        assert_eq!(e, 0);
        assert!(!output.contains("SECRET"));
    }
}
#[test]
fn diagnostic_input_is_explicit_and_bounded() {
    let f = Fixture::new();
    for (format, s, e) in [
        (
            "envelope",
            r#"{"schema_version":"wrong","diagnostics":[]}"#,
            3,
        ),
        ("validation-errors", r#"[{"code":"custom"}]"#, 2),
        ("cargo", "not json", 2),
        ("garbage", "[]", 2),
    ] {
        f.write("d.json", s);
        assert_eq!(
            run(&[
                "explain",
                "--diagnostics",
                &f.path("d.json"),
                "--input-format",
                format,
                "--core-version",
                "0.6.0"
            ])
            .0,
            e
        );
    }
    f.write("d.json",serde_json::to_vec(&json!({"schema_version":"rydantic.cli/v1","diagnostics":vec![json!({"code":"custom"});101]})).unwrap());
    assert_eq!(
        run(&[
            "explain",
            "--diagnostics",
            &f.path("d.json"),
            "--input-format",
            "envelope",
            "--core-version",
            "0.6.0"
        ])
        .0,
        7
    );
}
#[test]
fn deterministic_stdout() {
    let f = Fixture::new();
    f.write("Cargo.toml", manifest("[dependencies]\nrydantic='=0.6.0'"));
    assert_eq!(f.doctor(&[]).2, f.doctor(&[]).2);
}

#[test]
fn inherited_defaults_cannot_disable_workspace_defaults() {
    let f = Fixture::new();
    f.write("Cargo.toml", "[workspace]\nmembers=['a']\n[workspace.dependencies]\nrd={package='rydantic',version='=0.6.0'}\n");
    f.write("a/Cargo.toml", "[package]\nname='a'\nversion='0.1.0'\n[dependencies]\nrd={workspace=true,default-features=false}\n");
    let (e, v, _) = f.doctor(&[]);
    assert_eq!(e, 0);
    assert_eq!(
        v["data"]["declared_dependencies"][0]["declared_default_features"],
        true
    );
}
#[test]
fn alternative_registry_and_source_config_are_unknown() {
    let f = Fixture::new();
    f.write(
        "Cargo.toml",
        manifest(
            "[dependencies]\nrydantic={version='=0.6.0',features=['settings'],registry='private'}",
        ),
    );
    assert_eq!(f.doctor(&[]).0, 0);
    f.write(
        "Cargo.toml",
        manifest("[dependencies]\nrydantic={version='=0.6.0',features=['settings']}"),
    );
    f.write(
        ".cargo/config.toml",
        "[source.crates-io]\nreplace-with='SECRET'\n",
    );
    let (e, v, s) = f.doctor(&[]);
    assert_eq!(e, 0);
    assert!(!s.contains("SECRET"));
    assert_eq!(
        v["data"]["declared_dependencies"][0]["feature_knowledge"],
        "unknown"
    );
}
#[cfg(unix)]
#[test]
fn implicit_members_do_not_follow_external_symlinks() {
    let f = Fixture::new();
    let external = Fixture::new();
    external.write(
        "Cargo.toml",
        manifest("[dependencies]\nrydantic={version='=0.6.0',features=['settings']}"),
    );
    f.write("Cargo.toml", "[workspace]\nmembers=['outside']\n");
    std::os::unix::fs::symlink(&external.0, f.0.join("outside")).unwrap();
    assert_eq!(f.doctor(&[]).0, 5);
}
#[cfg(unix)]
#[test]
fn non_utf8_arguments_are_controlled_errors() {
    use std::os::unix::ffi::OsStringExt;
    let out = Command::new(env!("CARGO_BIN_EXE_rydantic"))
        .arg("doctor")
        .arg(std::ffi::OsString::from_vec(vec![255]))
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "RD_INPUT_ENCODING");
}
#[test]
fn packaged_schema_and_knowledge_contract() {
    let schema: Value = serde_json::from_str(include_str!("../protocol-v1.schema.json")).unwrap();
    let knowledge: Value =
        serde_json::from_str(include_str!("../knowledge/stable-0.6.0.json")).unwrap();
    let (_, v, _) = run(&["capabilities"]);
    for k in schema["required"].as_array().unwrap() {
        assert!(v.get(k.as_str().unwrap()).is_some());
    }
    for (code, entry) in knowledge["explanations"].as_object().unwrap() {
        let (e, v, _) = run(&["explain", code, "--core-version", "0.6.0"]);
        assert_eq!(e, 0);
        assert_eq!(v["data"]["explanations"][0]["known"], true);
        assert_eq!(
            v["data"]["explanations"][0]["next_action"],
            entry["next_action"]
        );
    }
}

#[cfg(unix)]
#[test]
fn implicit_member_manifest_cannot_escape_through_symlink() {
    let f = Fixture::new();
    let external = Fixture::new();
    external.write(
        "Cargo.toml",
        manifest("[dependencies]\nrydantic={version='=0.6.0',features=['settings']}"),
    );
    f.write("Cargo.toml", "[workspace]\nmembers=['member']\n");
    fs::create_dir(f.0.join("member")).unwrap();
    std::os::unix::fs::symlink(external.0.join("Cargo.toml"), f.0.join("member/Cargo.toml"))
        .unwrap();
    assert_eq!(f.doctor(&[]).0, 5);
}

#[test]
fn reserved_number_objects_cannot_impersonate_location_indices() {
    let f = Fixture::new();
    f.write("d.json",r#"[{"code":"custom","stage":"validate","loc":[{"$serde_json::private::Number":"42"}],"message":"SECRET"}]"#);
    let (e, _, s) = run(&[
        "explain",
        "--diagnostics",
        &f.path("d.json"),
        "--input-format",
        "validation-errors",
        "--core-version",
        "0.6.0",
    ]);
    assert_eq!(e, 2);
    assert!(!s.contains("SECRET"));
    f.write("Cargo.toml", manifest(""));
    for input in [
        r#"{"nested":{"$serde_json::private::Number":"1"}}"#,
        r#"{"nested":{"$serde_json::private::RawValue":"SECRET"}}"#,
        r#"{"nested":{"\u0024serde_json::private::Number":"1"}}"#,
    ] {
        f.write("s.json", input);
        assert_eq!(f.doctor(&["--schema", &f.path("s.json")]).0, 2);
    }
}

#[test]
fn cargo_home_ancestor_and_invocation_configs_make_feature_knowledge_unknown() {
    for config in [
        "home/config",
        "home/config.toml",
        ".cargo/config.toml",
        "consumer/nested/.cargo/config.toml",
    ] {
        let f = Fixture::new();
        f.write(
            "consumer/Cargo.toml",
            manifest("[dependencies]\nrydantic={version='=0.6.0',features=['settings']}"),
        );
        f.write(config, "paths=['SECRET_OVERRIDE']\n");
        fs::create_dir_all(f.0.join("consumer/nested")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_rydantic"));
        command
            .current_dir(f.0.join("consumer/nested"))
            .env("CARGO_HOME", f.0.join("home"));
        let (exit, value, output) =
            run_command(command, &["doctor", "--workspace", &f.path("consumer")]);
        assert_eq!(exit, 0);
        assert_eq!(value["data"]["cargo_config_present"], true);
        assert_eq!(
            value["data"]["declared_dependencies"][0]["feature_knowledge"],
            "unknown"
        );
        assert!(!output.contains("RD_FEATURE_UNAVAILABLE"));
        assert!(!output.contains("SECRET_OVERRIDE"));
    }
}

#[test]
fn inherited_2024_defaults_are_unknown_without_verified_cargo_version() {
    for inherited_edition in [false, true] {
        let f = Fixture::new();
        f.write("Cargo.toml", "[workspace]\nmembers=['a']\n[workspace.package]\nedition='2024'\n[workspace.dependencies]\nrd={package='rydantic',version='=0.6.0'}\n");
        let edition = if inherited_edition {
            "edition.workspace=true"
        } else {
            "edition='2024'"
        };
        f.write("a/Cargo.toml", format!("[package]\nname='a'\nversion='0.1.0'\n{edition}\n[dependencies]\nrd={{workspace=true,default-features=false}}\n"));
        let (exit, value, _) = f.doctor(&[]);
        assert_eq!(exit, 0);
        assert!(value["data"]["declared_dependencies"][0]["declared_default_features"].is_null());
        assert!(value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "RD_DEFAULT_FEATURES_UNKNOWN"));
        assert!(value["data"]["resolved_features"].is_null());
        assert_eq!(value["data"]["project_code_executed"], false);
    }
}

#[test]
fn home_fallback_missing_home_and_relative_cargo_home_fail_closed() {
    for mode in ["fallback", "missing", "relative"] {
        let f = Fixture::new();
        f.write(
            "Cargo.toml",
            manifest("[dependencies]\nrydantic={version='=0.6.0',features=['settings']}"),
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_rydantic"));
        command
            .current_dir(&f.0)
            .env_remove("CARGO_HOME")
            .env_remove("HOME")
            .env_remove("USERPROFILE");
        match mode {
            "fallback" => {
                f.write("home/.cargo/config.toml", "paths=['SECRET_OVERRIDE']\n");
                command.env(
                    if cfg!(windows) { "USERPROFILE" } else { "HOME" },
                    f.0.join("home"),
                );
            }
            "relative" => {
                f.write("relative/config", "paths=['SECRET_OVERRIDE']\n");
                command.env("CARGO_HOME", "relative");
            }
            _ => {}
        }
        let (exit, value, output) = run_command(command, &["doctor", "--workspace", &f.path("")]);
        assert_eq!(exit, 0, "{mode}");
        assert_eq!(
            value["data"]["cargo_config_present"],
            if mode == "missing" {
                Value::Null
            } else {
                json!(true)
            },
            "{mode}"
        );
        assert_eq!(
            value["data"]["declared_dependencies"][0]["feature_knowledge"], "unknown",
            "{mode}"
        );
        assert!(!output.contains("RD_FEATURE_UNAVAILABLE"));
        assert!(!output.contains("SECRET_OVERRIDE"));
    }
}

#[test]
fn inherited_defaults_preserve_unambiguous_additive_cases() {
    for (edition, workspace, member, expected) in [
        ("edition='2015'", true, Some(false), Some(true)),
        ("edition='2018'", true, Some(false), Some(true)),
        ("edition='2021'", true, Some(false), Some(true)),
        ("edition.workspace=true", true, Some(false), Some(true)),
        ("edition='2099'", true, Some(false), None),
        ("edition='2024'", false, Some(true), Some(true)),
        ("edition='2024'", false, Some(false), Some(false)),
        ("edition='2024'", false, None, Some(false)),
        ("edition='2024'", true, Some(true), Some(true)),
        ("edition='2024'", true, None, Some(true)),
    ] {
        let f = Fixture::new();
        f.write("Cargo.toml", format!("[workspace]\nmembers=['a']\n[workspace.package]\nedition='2021'\n[workspace.dependencies]\nrd={{package='rydantic',version='=0.6.0',default-features={workspace}}}\n"));
        let member = member
            .map(|v| format!(",default-features={v}"))
            .unwrap_or_default();
        f.write("a/Cargo.toml", format!("[package]\nname='a'\nversion='0.1.0'\n{edition}\n[dependencies]\nrd={{workspace=true{member}}}\n"));
        let (exit, value, _) = f.doctor(&[]);
        assert_eq!(exit, 0);
        assert_eq!(
            value["data"]["declared_dependencies"][0]["declared_default_features"],
            json!(expected),
            "{edition}, workspace={workspace}, member={member}"
        );
        assert_eq!(
            value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "RD_DEFAULT_FEATURES_UNKNOWN"),
            expected.is_none()
        );
    }
}

#[test]
fn exact_070_capabilities_and_explanations() {
    let (_, v, _) = run(&["capabilities"]);
    assert_eq!(v["data"]["knowledge_versions"], json!(["0.6.0", "0.7.0"]));
    let knowledge: Value =
        serde_json::from_str(include_str!("../knowledge/stable-0.7.0.json")).unwrap();
    for (code, entry) in knowledge["explanations"].as_object().unwrap() {
        let (exit, v, _) = run(&["explain", code, "--core-version", "0.7.0"]);
        assert_eq!(exit, 0);
        assert_eq!(v["data"]["core_version"], "0.7.0");
        assert_eq!(
            v["data"]["knowledge_version"],
            knowledge["knowledge_version"]
        );
        assert_eq!(v["data"]["explanations"][0]["message"], entry["message"]);
        assert_eq!(
            v["data"]["explanations"][0]["next_action"],
            entry["next_action"]
        );
    }
    let (exit, v, text) = run(&["explain", "SECRET-CALLBACK", "--core-version", "0.7.0"]);
    assert_eq!(exit, 0);
    assert_eq!(v["data"]["explanations"][0]["known"], false);
    assert!(!text.contains("SECRET-CALLBACK"));
    for version in [
        "0.7",
        "=0.7.0",
        "0.7.1",
        "0.7.0-beta.4",
        "0.8.0-beta.1",
        "0.7.0+local",
        " 0.7.0",
    ] {
        let (exit, v, _) = run(&["explain", "missing", "--core-version", version]);
        assert_eq!(exit, 3, "{version}");
        assert!(v["data"]["core_version"].is_null());
        assert_eq!(v["data"]["explanations"][0]["known"], false);
    }
}

#[test]
fn exact_070_features_are_version_specific_and_static() {
    let f = Fixture::new();
    f.write("Cargo.toml", manifest("[dependencies]\nrd={package='rydantic',version='=0.7.0',features=['settings','partial-json','phone','country','currency']}"));
    let (exit, v, _) = f.doctor(&[]);
    assert_eq!(exit, 0);
    let d = &v["data"]["declared_dependencies"][0];
    assert_eq!(d["feature_knowledge"], "exact_declaration_0.7.0");
    assert_eq!(d["version_association"], "unknown");
    assert!(d["verified_features"].is_null());
    assert!(!f.0.join("Cargo.lock").exists());
    assert!(!v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "RD_VERSION_UNKNOWN"));
    f.write(
        "Cargo.toml",
        manifest(
            "[dependencies]\nrydantic={version='=0.7.0',features=['openapi','not-a-feature']}",
        ),
    );
    let (exit, v, _) = f.doctor(&[]);
    assert_eq!(exit, 3);
    let d = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "RD_FEATURE_UNAVAILABLE")
        .unwrap();
    assert_eq!(d["params"]["core_version"], "0.7.0");
    assert_eq!(d["params"]["features"], json!(["not-a-feature", "openapi"]));
}

#[test]
fn mixed_exact_versions_select_per_declaration() {
    let f = Fixture::new();
    f.write("Cargo.toml", "[workspace]\nmembers=['a','b']\n[workspace.dependencies]\nrd={package='rydantic',version='=0.7.0',features=['settings']}\n");
    f.write("a/Cargo.toml", "[package]\nname='a'\nversion='0.1.0'\n[dependencies]\nrd.workspace=true\n[target.'cfg(unix)'.dev-dependencies]\nold={package='rydantic',version='=0.6.0',features=['settings']}\n");
    f.write("b/Cargo.toml", "[package]\nname='b'\nversion='0.1.0'\n[build-dependencies]\nrd={package='rydantic',version='=0.7.0',features=['partial-json']}\n");
    let (exit, v, _) = f.doctor(&[]);
    assert_eq!(exit, 3);
    let declarations = v["data"]["declared_dependencies"].as_array().unwrap();
    assert_eq!(declarations.len(), 3);
    for d in declarations {
        assert_eq!(
            d["feature_knowledge"],
            if d["dependency_name"] == "old" {
                "exact_declaration_0.6.0"
            } else {
                "exact_declaration_0.7.0"
            }
        );
    }
    let failures: Vec<_> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "RD_FEATURE_UNAVAILABLE")
        .collect();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["params"]["core_version"], "0.6.0");
}

#[test]
fn exact_070_source_and_range_uncertainty_stays_unknown() {
    for dep in [
        "version='0.7'",
        "version='0.7.0'",
        "version='>=0.7.0'",
        "version='=0.7.1'",
        "version='=0.7.0+local'",
        "version='=0.8.0-beta.1'",
        "version='=0.7.0',path='../local'",
        "version='=0.7.0',git='https://SECRET.invalid'",
        "version='=0.7.0',registry='private'",
    ] {
        let f = Fixture::new();
        f.write(
            "Cargo.toml",
            manifest(&format!(
                "[dependencies]\nrydantic={{{dep},features=['openapi']}}"
            )),
        );
        f.write("Cargo.lock", "version=4\n[[package]]\nname='rydantic'\nversion='0.7.0'\nsource='registry+https://SECRET.invalid'\n");
        let (exit, v, text) = f.doctor(&[]);
        assert_eq!(exit, 0, "{dep}");
        assert_eq!(
            v["data"]["declared_dependencies"][0]["feature_knowledge"], "unknown",
            "{dep}"
        );
        assert_eq!(v["data"]["resolution"], "unknown");
        assert!(!text.contains("SECRET"));
    }
    for suffix in [
        "[patch.crates-io]\nrydantic={path='../local'}",
        "[replace]\n'rydantic:0.7.0'={path='../local'}",
    ] {
        let f = Fixture::new();
        f.write(
            "Cargo.toml",
            manifest(&format!(
                "[dependencies]\nrydantic={{version='=0.7.0',features=['openapi']}}\n{suffix}"
            )),
        );
        let (exit, v, _) = f.doctor(&[]);
        assert_eq!(exit, 0);
        assert_eq!(
            v["data"]["declared_dependencies"][0]["feature_knowledge"],
            "unknown"
        );
    }
}

#[test]
fn exact_070_config_and_stale_locks_never_resolve() {
    let f = Fixture::new();
    f.write(
        "Cargo.toml",
        manifest("[dependencies]\nrydantic={version='=0.7.0',features=['settings']}"),
    );
    f.write("Cargo.lock", "version=4\n[[package]]\nname='rydantic'\nversion='0.6.0'\nsource='registry+https://SECRET.invalid'\n");
    let (exit, v, _) = f.doctor(&[]);
    assert_eq!(exit, 0);
    assert_eq!(
        v["data"]["declared_dependencies"][0]["feature_knowledge"],
        "exact_declaration_0.7.0"
    );
    assert_eq!(
        v["data"]["declared_dependencies"][0]["supported_lock_candidate"],
        false
    );
    assert!(v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "RD_LOCK_MISMATCH"));
    f.write("Cargo.lock", "version=4\n[[package]]\nname='rydantic'\nversion='0.7.0'\nsource='registry+https://SECRET.invalid'\n");
    let (_, v, _) = f.doctor(&[]);
    assert_eq!(
        v["data"]["declared_dependencies"][0]["supported_lock_candidate"],
        true
    );
    assert_eq!(
        v["data"]["declared_dependencies"][0]["version_association"],
        "unknown"
    );
    f.write(".cargo/config.toml", "SECRET");
    let (exit, v, text) = f.doctor(&[]);
    assert_eq!(exit, 0);
    assert_eq!(
        v["data"]["declared_dependencies"][0]["feature_knowledge"],
        "unknown"
    );
    assert!(!text.contains("SECRET"));
}
