use std::{
    io::Write,
    process::{Command, Stdio},
};
fn run(input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rydantic-codegen"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}
#[test]
fn cli_accepts_stdin_and_rejects_bad_input_without_partial_output() {
    let good = run(br#"{"type":"boolean"}"#);
    assert!(good.status.success());
    assert!(String::from_utf8(good.stdout)
        .unwrap()
        .contains("pub struct RootModel"));
    for bad in [
        br#"{"type":"string","type":"boolean"}"#.as_slice(),
        br#"{"type":"array","items":{"type":"string","type":"boolean"}}"#,
        br#"{"type":"number"}"#,
        br#"{"$ref":"https://example.invalid"}"#,
        br#"{"type":"string""#,
    ] {
        let output = run(bad);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let output = run(&vec![b' '; 65_537]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
