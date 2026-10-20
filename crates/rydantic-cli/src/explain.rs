use crate::{diagnostic, fail, input, Args, Report, Result, KNOWLEDGE, LIMIT};
use serde_json::{json, Value};
use std::path::Path;

pub fn run(a: &Args) -> Result<Report> {
    let version = a.get("--core-version").ok_or_else(|| {
        fail(
            2,
            "RD_USAGE_INVALID",
            "explain requires an explicit --core-version.",
        )
    })?;
    let mut codes = vec![];
    match (&a.code, a.get("--diagnostics"), a.get("--input-format")) {
        (Some(c), None, None) => codes.push(c.clone()),
        (None, Some(path), Some(format)) => {
            let raw = input::read(Path::new(path), LIMIT)?;
            match format {
                "envelope" => {
                    let v = input::parse_json(&raw)?;
                    if v.get("schema_version").and_then(Value::as_str) != Some("rydantic.cli/v1") {
                        return Err(fail(
                            3,
                            "RD_PROTOCOL_UNSUPPORTED",
                            "Diagnostic envelope protocol is unsupported.",
                        ));
                    }
                    collect(v.get("diagnostics"), &mut codes, false)?;
                }
                "validation-errors" => collect(Some(&input::parse_json(&raw)?), &mut codes, true)?,
                "cargo" => {
                    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
                        let v = input::parse_json(line)?;
                        if v.get("reason").and_then(Value::as_str) == Some("compiler-message") {
                            // Rust compiler codes cannot safely be attributed to Rydantic.
                            codes.push("unknown".into());
                        } else if v.get("reason").and_then(Value::as_str).is_none() {
                            return Err(invalid());
                        }
                        if codes.len() > 100 {
                            return Err(limit());
                        }
                    }
                }
                _ => return Err(invalid()),
            }
        }
        _ => return Err(invalid()),
    }
    if codes.len() > 100 {
        return Err(limit());
    }
    let known = version == "0.6.0";
    let knowledge: Value = serde_json::from_str(KNOWLEDGE).expect("bundled knowledge is valid");
    let mut explanations = vec![];
    for code in codes {
        if known {
            if let Some(e) = knowledge["explanations"].get(&code) {
                explanations.push(json!({"code":code,"known":true,"message":e["message"],"next_action":e["next_action"]}));
                continue;
            }
        }
        // User callbacks may put payloads even in their code, not only message/params.
        explanations.push(json!({"code":"unknown","known":false,"message":"No verified Rydantic explanation is available. Original diagnostic content is not echoed."}));
    }
    let mut report = Report::new(
        json!({"core_version":if known{json!(version)}else{Value::Null},"knowledge_version":if known{json!(1)}else{Value::Null},"explanations":explanations,"project_code_executed":false}),
    );
    if !known {
        report.exit = 3;
        report.diagnostics.push(diagnostic(
            "RD_VERSION_UNKNOWN",
            "error",
            "explain",
            "No verified knowledge exists for the requested exact core version.",
        ));
    }
    Ok(report)
}
fn invalid() -> crate::Failure {
    fail(2,"RD_DIAGNOSTICS_INVALID","Use one code or an explicit diagnostics file and input format; malformed diagnostics are rejected.")
}
fn limit() -> crate::Failure {
    fail(
        7,
        "RD_LIMIT_EXCEEDED",
        "At most 100 diagnostics can be explained per call.",
    )
}
fn collect(v: Option<&Value>, codes: &mut Vec<String>, validation: bool) -> Result<()> {
    let entries = v.and_then(Value::as_array).ok_or_else(invalid)?;
    if entries.len() > 100 {
        return Err(limit());
    }
    for e in entries {
        let code = e.get("code").and_then(Value::as_str).ok_or_else(invalid)?;
        if validation {
            let loc = e.get("loc").and_then(Value::as_array).ok_or_else(invalid)?;
            if !loc.iter().all(|p| p.is_string() || p.as_u64().is_some())
                || !matches!(
                    e.get("stage").and_then(Value::as_str),
                    Some("parse" | "validate" | "serialize")
                )
                || e.get("message").and_then(Value::as_str).is_none()
            {
                return Err(invalid());
            }
        }
        codes.push(code.into());
    }
    Ok(())
}
