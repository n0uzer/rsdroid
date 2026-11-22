//! Local-only doctor bundle: a ZIP with the report, environment and existing logs.
//!
//! Nothing is uploaded. The archive is written next to the user's working
//! directory (or the chosen output directory) so it can be attached to an issue
//! by hand.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use time::OffsetDateTime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::Report;
use crate::error::{io, Result};

const MAX_BUNDLED_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Where to put the bundle and which data directory to include.
#[derive(Debug, Clone)]
pub struct BundleOptions {
    /// Directory that receives `rsdroid-doctor-*.zip`.
    pub output_dir: PathBuf,
    /// Directory whose `logs/*.log` and `*.toml` files are included.
    pub data_dir: Option<PathBuf>,
}

impl BundleOptions {
    /// Build options for an output directory and an optional data directory.
    pub fn new(output_dir: impl Into<PathBuf>, data_dir: Option<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            data_dir,
        }
    }
}

/// Write a bundle and return its path. The file is created atomically.
pub fn write_bundle(report: &Report, options: &BundleOptions) -> Result<PathBuf> {
    fs::create_dir_all(&options.output_dir).map_err(|error| io(&options.output_dir, error))?;

    let stamp = utc_stamp();
    let path = unique_path(&options.output_dir, &format!("rsdroid-doctor-{stamp}.zip"));
    let temporary = path.with_extension("zip.part");
    let file = File::create(&temporary).map_err(|error| io(&temporary, error))?;
    let mut zip = ZipWriter::new(file);
    let entry = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    add_bytes(
        &mut zip,
        "doctor.json",
        &serde_json::to_vec_pretty(report)?,
        entry,
    )?;
    add_bytes(
        &mut zip,
        "doctor.txt",
        report.render_human().as_bytes(),
        entry,
    )?;
    add_bytes(
        &mut zip,
        "environment.txt",
        environment_text(report).as_bytes(),
        entry,
    )?;
    add_bytes(&mut zip, "README.txt", README.as_bytes(), entry)?;

    let mut skipped = Vec::new();
    if let Some(data_dir) = &options.data_dir {
        add_matching_files(
            &mut zip,
            &data_dir.join("logs"),
            "logs",
            "log",
            entry,
            &mut skipped,
        )?;
        add_matching_files(
            &mut zip,
            &data_dir.join("config"),
            "config",
            "toml",
            entry,
            &mut skipped,
        )?;
    }
    if !skipped.is_empty() {
        add_bytes(
            &mut zip,
            "skipped.txt",
            skipped.join("\n").as_bytes(),
            entry,
        )?;
    }

    zip.finish()?;
    fs::rename(&temporary, &path).map_err(|error| io(&path, error))?;
    Ok(path)
}

fn add_bytes(
    zip: &mut ZipWriter<File>,
    name: &str,
    bytes: &[u8],
    options: SimpleFileOptions,
) -> Result<()> {
    zip.start_file(name.to_owned(), options)?;
    zip.write_all(bytes).map_err(|error| io(name, error))?;
    Ok(())
}

fn add_file(
    zip: &mut ZipWriter<File>,
    name: &str,
    path: &Path,
    options: SimpleFileOptions,
) -> Result<()> {
    let mut file = File::open(path).map_err(|error| io(path, error))?;
    zip.start_file(name.to_owned(), options)?;
    io::copy(&mut file, zip).map_err(|error| io(path, error))?;
    Ok(())
}

fn add_matching_files(
    zip: &mut ZipWriter<File>,
    directory: &Path,
    prefix: &str,
    extension: &str,
    options: SimpleFileOptions,
    skipped: &mut Vec<String>,
) -> Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io(directory, error)),
    };

    for entry in entries {
        let entry = entry.map_err(|error| io(directory, error))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| io(&path, error))?;
        if !file_type.is_file()
            || path.extension().and_then(|value| value.to_str()) != Some(extension)
        {
            continue;
        }
        let size = entry.metadata().map_err(|error| io(&path, error))?.len();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if size > MAX_BUNDLED_FILE_BYTES {
            skipped.push(format!(
                "{prefix}/{file_name} skipped: {size} bytes exceeds the 16 MiB limit"
            ));
            continue;
        }
        add_file(zip, &format!("{prefix}/{file_name}"), &path, options)?;
    }
    Ok(())
}

fn environment_text(report: &Report) -> String {
    format!(
        "rsdroid doctor environment\n\
         generated_at: {}\n\
         os_name: {}\n\
         os_version: {}\n\
         os_build: {}\n\
         architecture: {}\n\
         cpu: {}\n\
         cpu_cores: {}\n\
         memory_mb: {}\n\
         hypervisor_present: {}\n",
        report.generated_at,
        report.platform.os_name,
        report.platform.os_version,
        report.platform.os_build,
        report.platform.architecture,
        report.platform.cpu,
        report.platform.cpu_cores,
        report.platform.memory_mb,
        report.platform.hypervisor_present
    )
}

fn utc_stamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn unique_path(directory: &Path, file_name: &str) -> PathBuf {
    let candidate = directory.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = file_name.strip_suffix(".zip").unwrap_or(file_name);
    for suffix in 2..1000 {
        let candidate = directory.join(format!("{stem}-{suffix}.zip"));
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{stem}-999.zip"))
}

const README: &str = "\
rsdroid doctor bundle
=====================

This archive was created locally by `rsdroid doctor --bundle`.
It was not uploaded anywhere. Review the contents before sharing it:
paths, usernames and environment details may be present.

Contents:
  doctor.json      machine-readable report (schema version 1)
  doctor.txt       the report exactly as printed
  environment.txt  host facts
  logs/            existing *.log files up to 16 MiB each
  config/          existing *.toml files up to 16 MiB each
  skipped.txt      present only when a file was too large
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::{Check, CheckId, CheckStatus, PlatformInfo, Report};
    use std::io::Read;

    fn report() -> Report {
        let platform = PlatformInfo {
            os_name: "Windows 11 Pro".to_owned(),
            os_version: "24H2 (26100.1)".to_owned(),
            os_build: 26100,
            architecture: "x86_64".to_owned(),
            cpu: "Test CPU".to_owned(),
            cpu_cores: 8,
            memory_mb: 16384,
            hypervisor_present: true,
        };
        Report::new(
            platform,
            vec![Check::new(CheckId::Qemu, CheckStatus::Ok, "QEMU", "test")],
        )
    }

    #[test]
    fn bundle_contains_report_logs_and_config() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        fs::create_dir_all(data.join("logs")).unwrap();
        fs::create_dir_all(data.join("config")).unwrap();
        fs::write(data.join("logs").join("rsdroid.log"), b"log line\n").unwrap();
        fs::write(data.join("config").join("settings.toml"), b"key = 1\n").unwrap();

        let output = temp.path().join("out");
        let bundle = write_bundle(&report(), &BundleOptions::new(&output, Some(data))).unwrap();
        assert!(bundle.exists());

        let mut archive = zip::ZipArchive::new(File::open(&bundle).unwrap()).unwrap();
        let mut names: Vec<String> = archive
            .file_names()
            .filter_map(|name| name.ok())
            .map(|name| name.into_owned())
            .collect();
        names.sort();
        for expected in [
            "README.txt",
            "config/settings.toml",
            "doctor.json",
            "doctor.txt",
            "environment.txt",
            "logs/rsdroid.log",
        ] {
            assert!(names.iter().any(|name| name == expected), "{expected}");
        }

        let mut text = String::new();
        archive
            .by_name("logs/rsdroid.log")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "log line\n");
    }

    #[test]
    fn second_bundle_gets_a_suffix() {
        let temp = tempfile::tempdir().unwrap();
        let options = BundleOptions::new(temp.path(), None);
        let first = write_bundle(&report(), &options).unwrap();
        let second = write_bundle(&report(), &options).unwrap();
        assert_ne!(first, second);
        assert!(second.to_string_lossy().contains("-2.zip"));
    }
}
