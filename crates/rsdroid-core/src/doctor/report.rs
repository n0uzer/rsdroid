//! Serializable doctor report and its human-readable rendering.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Version of the JSON report emitted by `rsdroid doctor --json`.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Outcome of one environment check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Everything is as expected.
    Ok,
    /// The environment works, but something should be reviewed.
    Warning,
    /// rsdroid cannot work until this is fixed.
    Error,
    /// The check did not run, usually because it does not apply here.
    Skipped,
}

impl CheckStatus {
    /// Short label used in the human-readable report.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warning => "warn",
            Self::Error => "fail",
            Self::Skipped => "skip",
        }
    }

    /// Whether this status makes the environment unusable.
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// Stable identifier of a doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckId {
    /// Windows version and edition.
    WindowsVersion,
    /// VT-x/AMD-V availability as seen by Windows.
    CpuVirtualization,
    /// Windows Hypervisor Platform (WHPX).
    HypervisorPlatform,
    /// QEMU executable, version and WHPX accelerator.
    Qemu,
    /// Android Debug Bridge availability.
    Adb,
    /// Other hypervisor or Android-emulator software.
    ConflictingSoftware,
    /// Writable rsdroid data directory.
    DataDirectory,
}

impl CheckId {
    /// Stable string form used in JSON and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WindowsVersion => "windows_version",
            Self::CpuVirtualization => "cpu_virtualization",
            Self::HypervisorPlatform => "hypervisor_platform",
            Self::Qemu => "qemu",
            Self::Adb => "adb",
            Self::ConflictingSoftware => "conflicting_software",
            Self::DataDirectory => "data_directory",
        }
    }
}

/// One doctor finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// Stable check identifier.
    pub id: CheckId,
    /// Outcome.
    pub status: CheckStatus,
    /// One-line summary.
    pub title: String,
    /// What was observed.
    pub detail: String,
    /// What the user can do about a warning or error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

impl Check {
    /// Create a check with an explicit status.
    pub fn new(
        id: CheckId,
        status: CheckStatus,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id,
            status,
            title: title.into(),
            detail: detail.into(),
            remediation: None,
        }
    }

    /// A passing check.
    pub fn ok(id: CheckId, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(id, CheckStatus::Ok, title, detail)
    }

    /// A non-blocking warning.
    pub fn warning(id: CheckId, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(id, CheckStatus::Warning, title, detail)
    }

    /// A blocking error.
    pub fn error(id: CheckId, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(id, CheckStatus::Error, title, detail)
    }

    /// A check that did not apply.
    pub fn skipped(id: CheckId, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(id, CheckStatus::Skipped, title, detail)
    }

    /// Attach an actionable hint.
    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }
}

/// Host facts collected before the checks run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformInfo {
    /// Product name, for example `Windows 11 Pro`.
    pub os_name: String,
    /// Display version and build, for example `24H2 (26100.1742)`.
    pub os_version: String,
    /// Windows build number.
    pub os_build: u32,
    /// Target architecture.
    pub architecture: String,
    /// CPU model string.
    pub cpu: String,
    /// Logical processor count.
    pub cpu_cores: u32,
    /// Physical memory in mebibytes.
    pub memory_mb: u64,
    /// Whether a hypervisor is present through WHPX.
    pub hypervisor_present: bool,
}

/// Counts per check status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Passing checks.
    pub ok: usize,
    /// Non-blocking warnings.
    pub warnings: usize,
    /// Blocking errors.
    pub errors: usize,
    /// Skipped checks.
    pub skipped: usize,
}

impl Summary {
    fn from_checks(checks: &[Check]) -> Self {
        let mut summary = Self {
            ok: 0,
            warnings: 0,
            errors: 0,
            skipped: 0,
        };
        for check in checks {
            match check.status {
                CheckStatus::Ok => summary.ok += 1,
                CheckStatus::Warning => summary.warnings += 1,
                CheckStatus::Error => summary.errors += 1,
                CheckStatus::Skipped => summary.skipped += 1,
            }
        }
        summary
    }
}

/// Complete result of `rsdroid doctor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// JSON schema version.
    pub schema_version: u32,
    /// UTC timestamp in RFC 3339 format.
    pub generated_at: String,
    /// Host facts.
    pub platform: PlatformInfo,
    /// Findings in display order.
    pub checks: Vec<Check>,
    /// Aggregated counts.
    pub summary: Summary,
}

impl Report {
    /// Build a report and compute its summary from the checks.
    pub fn new(platform: PlatformInfo, checks: Vec<Check>) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            generated_at: now_rfc3339(),
            summary: Summary::from_checks(&checks),
            platform,
            checks,
        }
    }

    /// Exit code promised by `PLAN.md`: 5 for an environment problem, 0 otherwise.
    pub fn exit_code(&self) -> u8 {
        if self.summary.errors > 0 {
            5
        } else {
            0
        }
    }

    /// Render the report for a terminal. ASCII only, so legacy consoles work.
    pub fn render_human(&self) -> String {
        let mut out = String::new();
        // Writing to a String cannot fail; the fmt result is intentionally ignored.
        let _ = writeln!(out, "rsdroid doctor {}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(
            out,
            "{} {} ({}) | {} logical cores | {} MB RAM",
            self.platform.os_name,
            self.platform.os_version,
            self.platform.architecture,
            self.platform.cpu_cores,
            self.platform.memory_mb
        );
        let _ = writeln!(out, "CPU: {}", self.platform.cpu);
        let _ = writeln!(
            out,
            "Hypervisor present: {}",
            if self.platform.hypervisor_present {
                "yes"
            } else {
                "no"
            }
        );
        let _ = writeln!(out);

        for check in &self.checks {
            let _ = writeln!(out, "[{:<4}] {}", check.status.label(), check.title);
            let _ = writeln!(out, "       {}", check.detail);
            if let Some(remediation) = &check.remediation {
                for line in remediation.lines() {
                    let _ = writeln!(out, "       -> {line}");
                }
            }
        }

        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Summary: {} ok, {} warning(s), {} error(s), {} skipped",
            self.summary.ok, self.summary.warnings, self.summary.errors, self.summary.skipped
        );
        if self.summary.errors > 0 {
            let _ = writeln!(
                out,
                "Result: environment is not ready; fix the errors above before starting a VM."
            );
        } else if self.summary.warnings > 0 {
            let _ = writeln!(
                out,
                "Result: environment is usable; review the warnings above."
            );
        } else {
            let _ = writeln!(out, "Result: environment is ready.");
        }
        out
    }
}

/// Current UTC time in RFC 3339 with second precision, for example
/// `2026-11-22T13:42:13Z`.
fn now_rfc3339() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform() -> PlatformInfo {
        PlatformInfo {
            os_name: "Windows 11 Pro".to_owned(),
            os_version: "24H2 (26100.1)".to_owned(),
            os_build: 26100,
            architecture: "x86_64".to_owned(),
            cpu: "Test CPU".to_owned(),
            cpu_cores: 8,
            memory_mb: 16384,
            hypervisor_present: true,
        }
    }

    fn check(status: CheckStatus) -> Check {
        Check::new(CheckId::Qemu, status, "QEMU", "test detail")
    }

    #[test]
    fn summary_counts_each_status() {
        let report = Report::new(
            platform(),
            vec![
                check(CheckStatus::Ok),
                check(CheckStatus::Warning),
                check(CheckStatus::Error),
                check(CheckStatus::Skipped),
            ],
        );
        assert_eq!(
            report.summary,
            Summary {
                ok: 1,
                warnings: 1,
                errors: 1,
                skipped: 1
            }
        );
    }

    #[test]
    fn errors_map_to_exit_code_five() {
        let ok = Report::new(platform(), vec![check(CheckStatus::Ok)]);
        assert_eq!(ok.exit_code(), 0);
        let failed = Report::new(platform(), vec![check(CheckStatus::Error)]);
        assert_eq!(failed.exit_code(), 5);
    }

    #[test]
    fn human_report_includes_remediation() {
        let report = Report::new(
            platform(),
            vec![check(CheckStatus::Error).with_remediation("install QEMU\nrun doctor again")],
        );
        let text = report.render_human();
        assert!(text.contains("[fail] QEMU"));
        assert!(text.contains("-> install QEMU"));
        assert!(text.contains("-> run doctor again"));
        assert!(text.contains("1 error(s)"));
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = Report::new(platform(), vec![check(CheckStatus::Ok)]);
        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, report);
        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"id\": \"qemu\""));
    }
}
