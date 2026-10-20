#![cfg(all(feature = "settings", feature = "derive"))]

use rydantic::{
    json,
    settings::{Settings, SettingsError, ValueMode},
    Model, ValidationError,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rydantic-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Debug, PartialEq, Model)]
struct Database {
    host: String,
    #[model(default = 5432)]
    port: u16,
}
#[derive(Debug, PartialEq, Model)]
struct Config {
    database: Database,
    #[model(default = 2, ge = 1)]
    workers: u16,
    #[model(default)]
    tags: Vec<String>,
    token: Option<String>,
}
#[derive(Debug, PartialEq, Model)]
struct Text {
    value: String,
}
fn text(settings: Settings) -> String {
    settings.load::<Text>().unwrap().value
}

#[test]
fn explicit_order_recursive_merge_and_model_defaults() {
    let cfg: Config = Settings::new()
        .json(json!({"database":{"host":"file","port":8000},"tags":["old","two"]}))
        .unwrap()
        .pairs(
            [("APP_DATABASE__HOST", "env"), ("UNRELATED", "ignored")],
            "APP_",
            ValueMode::JsonOrString,
        )
        .unwrap()
        .json(json!({"tags":["new"],"workers":4}))
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(
        cfg.database,
        Database {
            host: "env".into(),
            port: 8000
        }
    );
    assert_eq!(cfg.tags, ["new"]);
    assert_eq!(cfg.workers, 4);
    assert_eq!(cfg.token, None);
    let cfg: Config = Settings::new()
        .json(json!({"database":{"host":"base"}}))
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(cfg.workers, 2);
    assert_eq!(cfg.database.port, 5432);
}

#[test]
fn arrays_null_and_scalar_object_replacement_are_not_deep_element_merges() {
    let cfg: Config = Settings::new()
        .json(json!({"database":true,"token":"secret","tags":["a","b"]}))
        .unwrap()
        .json(json!({"database":{"host":"new"},"token":null,"tags":[]}))
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(cfg.token, None);
    assert!(cfg.tags.is_empty());
    assert!(Settings::new()
        .json(json!({"database":{"host":"old"}}))
        .unwrap()
        .json(json!({"database":null}))
        .unwrap()
        .load::<Config>()
        .is_err());
}

#[test]
fn source_order_can_reverse_conventional_environment_priority() {
    let value = Settings::new()
        .pairs([("APP_VALUE", "env")], "APP_", ValueMode::String)
        .unwrap()
        .json(json!({"value":"explicit"}))
        .unwrap();
    assert_eq!(text(value), "explicit");
}

#[test]
fn modes_are_explicit_for_json_looking_secrets() {
    assert_eq!(
        text(
            Settings::new()
                .pairs([("APP_VALUE", "true")], "APP_", ValueMode::String)
                .unwrap()
        ),
        "true"
    );
    assert!(Settings::new()
        .pairs([("APP_VALUE", "true")], "APP_", ValueMode::JsonOrString)
        .unwrap()
        .load::<Text>()
        .is_err());
    assert_eq!(
        text(
            Settings::new()
                .pairs([("APP_VALUE", r#""true""#)], "APP_", ValueMode::Json)
                .unwrap()
        ),
        "true"
    );
    assert_eq!(
        Settings::new()
            .pairs([("APP_VALUE", "oops")], "APP_", ValueMode::Json)
            .unwrap_err(),
        SettingsError::Syntax
    );
    assert_eq!(
        text(
            Settings::new()
                .pairs(
                    [("APP_VALUE", "[malformed")],
                    "APP_",
                    ValueMode::JsonOrString
                )
                .unwrap()
        ),
        "[malformed"
    );
}

#[test]
fn key_collisions_and_parent_child_conflicts_are_order_independent() {
    for pairs in [
        vec![("APP_DB", "{}"), ("APP_DB__HOST", "x")],
        vec![("APP_DB__HOST", "x"), ("APP_DB", "{}")],
        vec![("APP_VALUE", "a"), ("APP_value", "b")],
        vec![("APP_VALUE", "a"), ("APP_VALUE", "a")],
    ] {
        assert_eq!(
            Settings::new()
                .pairs(pairs, "APP_", ValueMode::JsonOrString)
                .unwrap_err(),
            SettingsError::ConflictingKeys
        );
    }
    for key in [
        "APP_",
        "APP___BAD",
        "APP_DB__",
        "APP_DB____HOST",
        "APP_1BAD",
        "APP_A/B",
        "APP_你好",
    ] {
        assert_eq!(
            Settings::new()
                .pairs([(key, "x")], "APP_", ValueMode::String)
                .unwrap_err(),
            SettingsError::InvalidKey
        );
    }
    assert_eq!(
        Settings::new()
            .pairs([("X", "x")], "", ValueMode::String)
            .unwrap_err(),
        SettingsError::InvalidKey
    );
    assert!(Settings::new()
        .pairs([("unrelated/invalid", "x")], "APP_", ValueMode::String)
        .is_ok());
}

#[test]
fn json_file_and_missing_or_nonobject_sources() {
    let tmp = Temp::new();
    let path = tmp.file("config.json", r#"{"database":{"host":"localhost"}}"#);
    assert_eq!(
        Settings::new()
            .json_file(path)
            .unwrap()
            .load::<Config>()
            .unwrap()
            .database
            .host,
        "localhost"
    );
    for value in [json!(null), json!([]), json!(1)] {
        assert_eq!(
            Settings::new().json(value).unwrap_err(),
            SettingsError::RootNotObject
        );
    }
    assert_eq!(
        Settings::new()
            .json_file(tmp.0.join("missing"))
            .unwrap_err(),
        SettingsError::Io(std::io::ErrorKind::NotFound)
    );
    assert_eq!(
        Settings::new()
            .json_file(tmp.file("bad", "secret{"))
            .unwrap_err(),
        SettingsError::Syntax
    );
    assert_eq!(
        Settings::new().json_file(&tmp.0).unwrap_err(),
        SettingsError::NotRegularFile
    );
}

#[test]
fn dotenv_grammar_quoting_comments_crlf_and_no_interpolation() {
    let tmp = Temp::new();
    for (input, expected) in [
        (
            " # comment\r\n\r\n APP_VALUE = plain # literal \r\n",
            "plain # literal",
        ),
        ("APP_VALUE='${NOT_EXPANDED}\\raw'\n", "${NOT_EXPANDED}\\raw"),
        (r#"APP_VALUE="escaped\nline""#, "escaped\nline"),
        ("APP_VALUE=\n", ""),
    ] {
        assert_eq!(
            text(
                Settings::new()
                    .dotenv_file(tmp.file("input.env", input), "APP_", ValueMode::String)
                    .unwrap()
            ),
            expected
        );
    }
    let cfg: Config = Settings::new()
        .dotenv_file(
            tmp.file("typed.env", "APP_DATABASE__HOST=local\nAPP_WORKERS=7"),
            "APP_",
            ValueMode::JsonOrString,
        )
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(cfg.workers, 7);
}

#[test]
fn dotenv_rejects_unsupported_syntax_even_for_unselected_keys() {
    let tmp = Temp::new();
    for input in [
        "export APP_VALUE=x",
        "APP_VALUE='unterminated",
        "APP_VALUE=\"x\" # inline",
        "APP_VALUE='multi\nline'",
        "OTHER=\"bad",
        "OTHER MISSING_EQUALS",
        "APP_VALUE='a'b'",
    ] {
        assert!(
            Settings::new()
                .dotenv_file(tmp.file("bad.env", input), "APP_", ValueMode::String)
                .is_err(),
            "{input}"
        );
    }
}

#[test]
fn secrets_preserve_whitespace_except_single_final_newline() {
    let tmp = Temp::new();
    tmp.file("VALUE", "  true \n\n");
    assert_eq!(
        text(
            Settings::new()
                .secrets_dir(&tmp.0, ValueMode::String)
                .unwrap()
        ),
        "  true \n"
    );
    tmp.file("VALUE", "abc\r\n");
    assert_eq!(
        text(
            Settings::new()
                .secrets_dir(&tmp.0, ValueMode::String)
                .unwrap()
        ),
        "abc"
    );
    fs::create_dir(tmp.0.join("subdir")).unwrap();
    assert_eq!(
        Settings::new()
            .secrets_dir(&tmp.0, ValueMode::String)
            .unwrap_err(),
        SettingsError::NotRegularFile
    );
}

#[test]
fn secrets_nested_names_and_collisions() {
    let tmp = Temp::new();
    tmp.file("DATABASE__HOST", "host\n");
    tmp.file("DATABASE__PORT", "5433\n");
    assert_eq!(
        Settings::new()
            .secrets_dir(&tmp.0, ValueMode::JsonOrString)
            .unwrap()
            .load::<Config>()
            .unwrap()
            .database
            .port,
        5433
    );
    tmp.file("database", "{}");
    assert_eq!(
        Settings::new()
            .secrets_dir(&tmp.0, ValueMode::JsonOrString)
            .unwrap_err(),
        SettingsError::ConflictingKeys
    );
}

#[test]
fn invalid_utf8_is_redacted() {
    let tmp = Temp::new();
    let path = tmp.0.join("VALUE");
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert_eq!(
        Settings::new().json_file(&path).unwrap_err(),
        SettingsError::InvalidUnicode
    );
    assert_eq!(
        Settings::new()
            .secrets_dir(&tmp.0, ValueMode::String)
            .unwrap_err(),
        SettingsError::InvalidUnicode
    );
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_secret_sources() {
    let tmp = Temp::new();
    let source = Temp::new();
    let target = source.file("value", "secret");
    std::os::unix::fs::symlink(target, tmp.0.join("VALUE")).unwrap();
    assert_eq!(
        Settings::new()
            .secrets_dir(&tmp.0, ValueMode::String)
            .unwrap_err(),
        SettingsError::NotRegularFile
    );
    assert_eq!(
        Settings::new().json_file(tmp.0.join("VALUE")).unwrap_err(),
        SettingsError::NotRegularFile
    );
}

fn reject_secret(value: &str) -> Result<(), ValidationError> {
    Err(ValidationError::new(value, value)
        .at(value.to_owned())
        .with_param("secret", value.to_owned()))
}
#[derive(Debug, Model)]
struct Sensitive {
    #[model(validate = reject_secret)]
    value: String,
}
#[test]
fn builder_and_every_error_surface_redact_even_custom_validator_details() {
    let settings = Settings::new()
        .json(json!({"value":"synthetic-secret-marker"}))
        .unwrap();
    assert_eq!(format!("{settings:?}"), "Settings { values: [REDACTED] }");
    let error = settings.load::<Sensitive>().unwrap_err();
    assert_eq!(error, SettingsError::Validation { count: 1 });
    assert!(!format!("{error:?} {error}").contains("synthetic-secret-marker"));
    assert!(std::error::Error::source(&error).is_none());
    let error = Settings::new()
        .json_file(Path::new("synthetic-secret-marker/no-file"))
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains("synthetic-secret-marker"));
}

#[test]
fn environment_source_in_isolated_process() {
    // Child processes contain ONLY synthetic variables; never change/read the
    // parent's real settings or secrets. The sole parent lookup is this marker.
    const MARKER: &str = "RYDANTIC_SETTINGS_TEST_CHILD";
    if let Some(mode) = std::env::var_os(MARKER) {
        if mode == "valid" {
            let cfg: Config = Settings::new()
                .env("APP_", ValueMode::JsonOrString)
                .unwrap()
                .load()
                .unwrap();
            assert_eq!(cfg.database.host, "synthetic-host");
            assert_eq!(cfg.workers, 7);
        } else {
            assert_eq!(
                Settings::new().env("APP_", ValueMode::String).unwrap_err(),
                SettingsError::InvalidUnicode
            );
        }
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "environment_source_in_isolated_process",
            "--nocapture",
        ])
        .env_clear()
        .env(MARKER, "valid")
        .env("APP_DATABASE__HOST", "synthetic-host")
        .env("APP_WORKERS", "7")
        .env("app_workers", "ignored")
        .env("UNRELATED", "ignored");
    assert!(child.status().unwrap().success());
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        for invalid_name in [false, true] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "environment_source_in_isolated_process",
                    "--nocapture",
                ])
                .env_clear()
                .env(MARKER, "invalid");
            if invalid_name {
                child.env(OsString::from_vec(b"APP_\xff".to_vec()), "synthetic");
            } else {
                child.env("APP_VALUE", OsString::from_vec(vec![0xff]));
            }
            assert!(child.status().unwrap().success());
        }
    }
}

#[test]
fn nested_key_depth_boundary_is_explicit() {
    let key = |depth| format!("APP_{}", vec!["NODE"; depth].join("__"));
    assert!(Settings::new()
        .pairs([(key(64), "x")], "APP_", ValueMode::String)
        .is_ok());
    assert_eq!(
        Settings::new()
            .pairs([(key(65), "x")], "APP_", ValueMode::String)
            .unwrap_err(),
        SettingsError::InvalidKey
    );
}

#[test]
fn ordered_file_secrets_dotenv_and_explicit_layers() {
    let files = Temp::new();
    let secrets = Temp::new();
    let config = files.file("config.json", r#"{"value":"json"}"#);
    secrets.file("VALUE", "secrets\n");
    let dotenv = files.file("input.env", "APP_VALUE=dotenv\n");
    let settings = Settings::new()
        .json_file(config)
        .unwrap()
        .secrets_dir(&secrets.0, ValueMode::String)
        .unwrap();
    assert_eq!(text(settings.clone()), "secrets");
    let settings = settings
        .dotenv_file(dotenv, "APP_", ValueMode::String)
        .unwrap();
    assert_eq!(text(settings.clone()), "dotenv");
    assert_eq!(
        text(
            settings
                .pairs([("APP_VALUE", "pairs")], "APP_", ValueMode::String)
                .unwrap()
        ),
        "pairs"
    );
}

#[derive(Debug, Model)]
struct Aliased {
    #[model(validation_alias = "server_port", coerce, ge = 1)]
    port: u16,
}
#[test]
fn aliases_and_explicit_coercion_use_existing_model_rules() {
    let config: Aliased = Settings::new()
        .pairs([("APP_SERVER_PORT", "8080")], "APP_", ValueMode::String)
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(config.port, 8080);
    assert!(Settings::new()
        .pairs([("APP_SERVER_PORT", "0")], "APP_", ValueMode::String)
        .unwrap()
        .load::<Aliased>()
        .is_err());
}

#[test]
fn dotenv_rejects_bare_carriage_returns_before_trimming() {
    let tmp = Temp::new();
    for input in [
        "APP_VALUE=ok\rBROKEN",
        "APP_VALUE=ok\r",
        "# comment\r",
        "APP_VALUE='a\rb'",
        "APP_VALUE=\r\r\n",
    ] {
        assert_eq!(
            Settings::new()
                .dotenv_file(tmp.file("bare-cr.env", input), "APP_", ValueMode::String)
                .unwrap_err(),
            SettingsError::Syntax
        );
    }
    assert_eq!(
        text(
            Settings::new()
                .dotenv_file(
                    tmp.file("escaped.env", r#"APP_VALUE="a\rb""#),
                    "APP_",
                    ValueMode::String
                )
                .unwrap()
        ),
        "a\rb"
    );
}
