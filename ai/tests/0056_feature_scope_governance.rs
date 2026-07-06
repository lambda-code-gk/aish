//! Feature scope governance acceptance tests (spec 0056).
//!
//! Runs `scripts/check-feature-scope.py` against fixture mini-repos.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn checker() -> PathBuf {
    repo_root().join("scripts/check-feature-scope.py")
}

fn fixture(name: &str) -> PathBuf {
    repo_root()
        .join("scripts/fixtures/feature-scope")
        .join(name)
}

fn run_checker(fixture_root: &Path, extra_args: &[&str]) -> (i32, String) {
    let mut cmd = Command::new("python3");
    cmd.arg(checker())
        .arg("--root")
        .arg(fixture_root)
        .arg("--skip-template");
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("spawn check-feature-scope.py");
    let code = output.status.code().unwrap_or(-1);
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (code, combined)
}

fn run_checker_main(extra_args: &[&str]) -> (i32, String) {
    let mut cmd = Command::new("python3");
    cmd.arg(checker());
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd
        .output()
        .expect("spawn check-feature-scope.py on main repo");
    let code = output.status.code().unwrap_or(-1);
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (code, combined)
}

#[test]
fn scope_template_required_sections() {
    let template = repo_root().join("docs/spec/_feature-spec-template.md");
    let text = std::fs::read_to_string(&template).expect("read template");
    for section in [
        "Core outcome",
        "Minimum vertical slice",
        "Fault model",
        "Non-goals",
        "Complexity inventory",
        "Complexity Gate",
        "Complexity budget",
        "Split triggers",
        "Acceptance Criteria",
        "Deferred specs",
        "Scope change log",
    ] {
        assert!(
            text.contains(section),
            "template missing section heading: {section}"
        );
    }
}

#[test]
fn scope_registry_requires_new_specs() {
    let (code, out) = run_checker(&fixture("missing-registry"), &[]);
    assert_ne!(
        code, 0,
        "expected failure for missing registry entry: {out}"
    );
    assert!(
        out.contains("requires feature entry"),
        "unexpected output: {out}"
    );
}

#[test]
fn scope_checker_accepts_valid_green_spec() {
    let (code, out) = run_checker(&fixture("valid-green"), &[]);
    assert_eq!(code, 0, "valid-green fixture should pass: {out}");
    assert!(
        out.contains("gate=Green"),
        "valid-green should report Green gate: {out}"
    );
}

#[test]
fn scope_checker_classifies_yellow() {
    let (code, out) = run_checker(&fixture("valid-yellow"), &[]);
    assert_eq!(code, 0, "valid-yellow fixture should pass: {out}");
    assert!(
        out.contains("gate=Yellow"),
        "valid-yellow should report Yellow gate: {out}"
    );
}

#[test]
fn scope_checker_rejects_red_feature() {
    let (code, out) = run_checker(&fixture("red-feature"), &[]);
    assert_ne!(code, 0, "red feature should fail: {out}");
    assert!(
        out.contains("Red complexity not allowed"),
        "unexpected output: {out}"
    );
}

#[test]
fn scope_checker_requires_yellow_review() {
    let (code, out) = run_checker(&fixture("yellow-without-review"), &[]);
    assert_ne!(code, 0, "yellow without review should fail: {out}");
    assert!(
        out.contains("scope_review=approved"),
        "unexpected output: {out}"
    );
}

#[test]
fn scope_lock_matches_acceptance_ids() {
    let (code, out) = run_checker(&fixture("ac-mismatch"), &[]);
    assert_ne!(code, 0, "ac mismatch should fail: {out}");
    assert!(
        out.contains("locked_ac_ids") || out.contains("spec-acceptance.toml"),
        "unexpected output: {out}"
    );
}

#[test]
fn scope_revision_required_on_locked_change() {
    let base = fixture("revision-not-incremented").join("base-registry/feature-scope.toml");
    let base_arg = base.to_string_lossy();
    let (code, out) = run_checker(
        &fixture("revision-not-incremented"),
        &["--base-registry", &base_arg],
    );
    assert_ne!(code, 0, "revision not incremented should fail: {out}");
    assert!(
        out.contains("scope_revision not increased"),
        "unexpected output: {out}"
    );
}

#[test]
fn scope_status_cannot_downgrade_from_locked() {
    let base = fixture("status-downgrade").join("base-registry/feature-scope.toml");
    let base_arg = base.to_string_lossy();
    let (code, out) = run_checker(
        &fixture("status-downgrade"),
        &["--base-registry", &base_arg],
    );
    assert_ne!(code, 0, "status downgrade should fail: {out}");
    assert!(
        out.contains("cannot downgrade status"),
        "unexpected output: {out}"
    );
}

#[test]
fn scope_vertical_slice_required() {
    let (code, out) = run_checker(&fixture("missing-vertical-slice"), &[]);
    assert_ne!(code, 0, "missing vertical slice should fail: {out}");
    assert!(
        out.contains("vertical_slice_ac_id"),
        "unexpected output: {out}"
    );
}

#[test]
fn scope_existing_specs_are_grandfathered() {
    let (code, out) = run_checker(&fixture("grandfathered-old-spec"), &[]);
    assert_eq!(code, 0, "grandfathered old spec should pass: {out}");
}

#[test]
fn scope_checker_runs_in_verify() {
    let verify =
        std::fs::read_to_string(repo_root().join("scripts/verify.sh")).expect("read verify.sh");
    assert!(
        verify.contains("check-feature-scope.py"),
        "verify.sh must invoke check-feature-scope.py"
    );
    let (code, out) = run_checker_main(&[]);
    assert_eq!(code, 0, "main repo checker should pass: {out}");
}

#[test]
fn scope_checker_rejects_missing_required_section_fixture() {
    let (code, out) = run_checker(&fixture("missing-required-section"), &[]);
    assert_ne!(code, 0, "missing section should fail: {out}");
    assert!(out.contains("Core outcome"), "unexpected output: {out}");
}

#[test]
fn scope_checker_accepts_red_platform_fixture() {
    let (code, out) = run_checker(&fixture("red-platform"), &[]);
    assert_eq!(code, 0, "red platform with exception should pass: {out}");
}
