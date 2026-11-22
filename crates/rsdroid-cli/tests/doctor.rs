#![allow(clippy::unwrap_used)] // Integration test code: every unwrap is a test assertion.

use std::fs::File;
use std::io::Read;

use assert_cmd::Command;
use predicates::prelude::*;

fn doctor_command() -> (Command, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("rsdroid").unwrap();
    command
        .arg("doctor")
        .arg("--data-dir")
        .arg(temp.path())
        .timeout(std::time::Duration::from_secs(60));
    (command, temp)
}

#[test]
fn help_lists_doctor() {
    Command::cargo_bin("rsdroid")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn doctor_json_is_machine_readable() {
    let (mut command, _temp) = doctor_command();
    let output = command.arg("--json").output().unwrap();
    assert!(
        matches!(output.status.code(), Some(0) | Some(5)),
        "unexpected exit code: {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(value["checks"]
        .as_array()
        .is_some_and(|checks| !checks.is_empty()));
    assert!(value["summary"]["errors"].as_u64().is_some());
}

#[test]
fn doctor_bundle_creates_a_zip() {
    let (mut command, temp) = doctor_command();
    let output_dir = temp.path().join("bundle");
    let output = command
        .arg("--json")
        .arg("--bundle")
        .arg("--bundle-dir")
        .arg(&output_dir)
        .output()
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0) | Some(5)),
        "unexpected exit code: {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let entries: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("zip"))
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly one bundle");

    let mut archive = zip::ZipArchive::new(File::open(&entries[0]).unwrap()).unwrap();
    let mut report = String::new();
    archive
        .by_name("doctor.json")
        .unwrap()
        .read_to_string(&mut report)
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&report).unwrap();
    assert_eq!(value["schema_version"], 1);
}
