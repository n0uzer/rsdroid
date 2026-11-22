#![allow(clippy::unwrap_used)] // Integration test code: every unwrap is a test assertion.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

fn command() -> (Command, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("rsdroid").unwrap();
    command.timeout(std::time::Duration::from_secs(30));
    (command, temp)
}

#[test]
fn stop_unknown_instance_exits_3() {
    let (mut command, temp) = command();
    command
        .arg("stop")
        .arg("missing")
        .arg("--data-dir")
        .arg(temp.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains("is not running"));
}

#[test]
fn run_rejects_an_unsafe_name_before_looking_for_tools() {
    let (mut command, temp) = command();
    command
        .arg("run")
        .arg("..")
        .arg("--data-dir")
        .arg(temp.path())
        .assert()
        .code(5)
        .stderr(predicate::str::contains("invalid VM configuration"));
}

#[test]
fn dry_run_prints_the_verified_command_line() {
    let (mut command, temp) = command();
    let image_dir = temp.path().join("images").join("android-x86_64-9.0-r2");
    fs::create_dir_all(&image_dir).unwrap();
    let iso = temp.path().join("images").join("android-x86_64-9.0-r2.iso");
    fs::write(&iso, b"iso").unwrap();
    let kernel = image_dir.join("kernel");
    let initrd = image_dir.join("initrd.img");
    let qemu = temp.path().join("qemu-system-x86_64.exe");
    let adb = temp.path().join("adb.exe");
    for file in [&kernel, &initrd, &qemu, &adb] {
        fs::write(file, b"placeholder").unwrap();
    }

    command
        .arg("run")
        .arg("demo")
        .arg("--dry-run")
        .arg("--qemu")
        .arg(&qemu)
        .arg("--adb")
        .arg(&adb)
        .arg("--iso")
        .arg(&iso)
        .arg("--data-dir")
        .arg(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("-append"))
        .stdout(predicate::str::contains("VIRT_WIFI=0"))
        .stdout(predicate::str::contains(
            "hostfwd=tcp:127.0.0.1:15555-:5555",
        ));
}
