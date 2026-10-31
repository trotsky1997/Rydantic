mod doctor;
mod explain;
mod input;

use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::PathBuf,
};

pub const KNOWLEDGE_VERSIONS: &[&str] = &["0.6.0", "0.7.0", "0.8.0"];
pub fn knowledge(version: &str) -> Option<Value> {
    let raw = match version {
        "0.6.0" => include_str!("../knowledge/stable-0.6.0.json"),
        "0.7.0" => include_str!("../knowledge/stable-0.7.0.json"),
        "0.8.0" => include_str!("../knowledge/stable-0.8.0.json"),
        _ => return None,
    };
    Some(serde_json::from_str(raw).expect("bundled knowledge is valid"))
}
pub const LIMIT: usize = 1_048_576;
pub type Result<T> = std::result::Result<T, Failure>;

pub struct Failure {
    pub exit: u8,
    pub code: &'static str,
    pub message: &'static str,
}
pub fn fail(exit: u8, code: &'static str, message: &'static str) -> Failure {
    Failure {
        exit,
        code,
        message,
    }
}
pub fn diagnostic(code: &str, severity: &str, stage: &str, message: &str) -> Value {
    json!({"code":code,"severity":severity,"stage":stage,"message":message,"retryable":false})
}
pub struct Report {
    pub data: Value,
    pub diagnostics: Vec<Value>,
    pub exit: u8,
}
impl Report {
    fn new(data: Value) -> Self {
        Self {
            data,
            diagnostics: vec![],
            exit: 0,
        }
    }
}

#[derive(Default)]
pub struct Args {
    pub command: String,
    pub values: BTreeMap<String, String>,
    pub code: Option<String>,
}
impl Args {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
    pub fn workspace(&self) -> PathBuf {
        PathBuf::from(self.get("--workspace").unwrap_or("."))
    }
    fn parse(raw: &[String]) -> Result<Self> {
        let mut a = Args::default();
        let mut i = 0;
        while i < raw.len() {
            let arg = &raw[i];
            if arg.starts_with("--") {
                if arg == "--offline" {
                    if a.values.insert(arg.clone(), "true".into()).is_some() {
                        return Err(usage());
                    }
                } else {
                    if ![
                        "--format",
                        "--protocol-version",
                        "--workspace",
                        "--package",
                        "--schema",
                        "--core-version",
                        "--diagnostics",
                        "--input-format",
                    ]
                    .contains(&arg.as_str())
                    {
                        return Err(usage());
                    }
                    i += 1;
                    let val = raw
                        .get(i)
                        .filter(|v| !v.starts_with("--"))
                        .ok_or_else(usage)?;
                    if a.values.insert(arg.clone(), val.clone()).is_some() {
                        return Err(usage());
                    }
                }
            } else if a.command.is_empty() {
                a.command = arg.clone();
            } else if a.code.is_none() {
                a.code = Some(arg.clone());
            } else {
                return Err(usage());
            }
            i += 1;
        }
        if !["text", "json"].contains(&a.get("--format").unwrap_or("text")) {
            return Err(usage());
        }
        if a.get("--protocol-version").unwrap_or("1") != "1" {
            return Err(fail(
                3,
                "RD_PROTOCOL_UNSUPPORTED",
                "Only protocol version 1 is supported.",
            ));
        }
        let allowed: &[&str] = match a.command.as_str() {
            "capabilities" => &[],
            "doctor" => &["--workspace", "--package", "--schema"],
            "explain" => &["--core-version", "--diagnostics", "--input-format"],
            _ => return Err(usage()),
        };
        if a.values.keys().any(|k| {
            !["--format", "--protocol-version", "--offline"].contains(&k.as_str())
                && !allowed.contains(&k.as_str())
        }) {
            return Err(usage());
        }
        if a.command != "explain" && a.code.is_some() {
            return Err(usage());
        }
        Ok(a)
    }
}
fn usage() -> Failure {
    fail(2,"RD_USAGE_INVALID","Invalid invocation. Use capabilities, doctor [--workspace PATH] [--package NAME] [--schema FILE], or explain CODE --core-version VERSION (or --diagnostics FILE --input-format envelope|validation-errors|cargo). Common flags: --format text|json --protocol-version 1 --offline. Discovery is static only; --resolve and --config are not supported.")
}
fn run(a: &Args) -> Result<Report> {
    match a.command.as_str() {
        "capabilities" => Ok(Report::new(
            json!({"commands":["capabilities","doctor","explain"],"protocol_versions":[1],"knowledge_versions":KNOWLEDGE_VERSIONS,"linked_engines":{"core":null,"codegen":null,"backend":null},"typed_adapter":false,"resolve":false,"configuration":false,"offline":true,"project_code_executed":false,"schema_dialects":[],"schema_inspection":"annotations_only","limits":{"file_bytes":LIMIT,"schema_bytes":65536,"diagnostics":100},"exit_codes":{"0":"success","1":"check_failed_reserved","2":"invalid_input","3":"unsupported","4":"io","5":"environment","6":"execution_reserved","7":"resource_limit","8":"policy_reserved","70":"internal"}}),
        )),
        "doctor" => doctor::run(a),
        "explain" => explain::run(a),
        _ => Err(usage()),
    }
}
fn main() {
    let raw_os: Vec<_> = std::env::args_os().skip(1).collect();
    let invalid_encoding = raw_os.iter().any(|s| s.to_str().is_none());
    let raw: Vec<String> = raw_os
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    let as_json = raw.windows(2).any(|w| w[0] == "--format" && w[1] == "json");
    let command = raw
        .iter()
        .find(|x| ["doctor", "explain", "capabilities"].contains(&x.as_str()))
        .map(String::as_str)
        .unwrap_or("unknown");
    let parsed = if invalid_encoding {
        Err(fail(2, "RD_INPUT_ENCODING", "Arguments must be UTF-8."))
    } else {
        Args::parse(&raw)
    };
    let report = parsed.and_then(|a| run(&a)).unwrap_or_else(|e| Report {
        data: json!({}),
        diagnostics: vec![diagnostic(e.code, "error", "invocation", e.message)],
        exit: e.exit,
    });
    let envelope = json!({"schema_version":"rydantic.cli/v1","tool":{"name":"rydantic","version":env!("CARGO_PKG_VERSION")},"command":command,"ok":report.exit==0,"status":if report.exit==0{"success"}else{"failed"},"data":report.data,"diagnostics":report.diagnostics,"artifacts":[]});
    let output = if as_json {
        format!("{envelope}\n")
    } else {
        format!(
            "{}\n",
            serde_json::to_string_pretty(&envelope).expect("JSON value serializes")
        )
    };
    if io::stdout().lock().write_all(output.as_bytes()).is_err() {
        std::process::exit(4);
    }
    std::process::exit(report.exit.into());
}
