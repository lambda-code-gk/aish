#!/usr/bin/env python3
"""Verify feature scope registry, complexity gates, and design spec structure."""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]


GREEN_LIMITS: dict[str, int] = {
    "actors": 2,
    "state_machines": 1,
    "persistent_aggregates": 1,
    "external_effects": 2,
    "process_boundaries": 1,
    "integrations": 1,
    "novel_mechanisms": 1,
}

RED_FLAG_FIELDS = (
    "crash_recovery",
    "schema_migration",
    "secondary_agent_loop",
    "lease_or_heartbeat",
    "reconciler",
    "exactly_once",
)

SCOPE_WATCH_FIELDS = (
    "core_outcome",
    "actors",
    "state_machines",
    "persistent_aggregates",
    "external_effects",
    "process_boundaries",
    "novel_mechanisms",
    "integrations",
    "crash_recovery",
    "schema_migration",
    "secondary_agent_loop",
    "lease_or_heartbeat",
    "reconciler",
    "exactly_once",
    "vertical_slice_ac_id",
    "locked_ac_ids",
    "scope_class",
)

REQUIRED_SPEC_SECTIONS = (
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
)

VALID_STATUS = frozenset({"draft", "locked", "done"})
VALID_SCOPE_CLASS = frozenset({"feature", "platform", "governance"})

SPEC_FILE_RE = re.compile(r"^(\d{4})_.*-spec\.md$")
HEADING_RE = re.compile(r"^#{1,6}\s+(.+?)\s*$")

EXCLUDED_SPEC_FILES = frozenset(
    {
        "README.md",
        "_feature-spec-template.md",
    }
)


@dataclass
class Feature:
    raw: dict[str, Any]
    spec: str

    def get(self, key: str, default: Any = None) -> Any:
        return self.raw.get(key, default)


def load_toml(path: Path) -> dict[str, Any]:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def parse_features(data: dict[str, Any]) -> list[Feature]:
    return [Feature(raw=entry, spec=str(entry["spec"])) for entry in data.get("features", [])]


def metric_value(feature: Feature, metric: str) -> int:
    if metric == "integrations":
        return len(feature.get("integrations") or [])
    if metric == "novel_mechanisms":
        return len(feature.get("novel_mechanisms") or [])
    return int(feature.get(metric, 0))


def classify_complexity(feature: Feature, policy: dict[str, Any]) -> str:
    """Return Green, Yellow, or Red."""
    for flag in RED_FLAG_FIELDS:
        if bool(feature.get(flag)):
            return "Red"

    if metric_value(feature, "novel_mechanisms") >= 2:
        return "Red"

    metrics = (
        "actors",
        "state_machines",
        "persistent_aggregates",
        "external_effects",
        "process_boundaries",
        "integrations",
    )
    for metric in metrics:
        value = metric_value(feature, metric)
        red_key = f"red_{metric}"
        if red_key in policy and value >= int(policy[red_key]):
            return "Red"

    for metric in metrics:
        value = metric_value(feature, metric)
        yellow_key = f"yellow_{metric}"
        if yellow_key in policy and value >= int(policy[yellow_key]):
            return "Yellow"

    for metric, limit in GREEN_LIMITS.items():
        if metric_value(feature, metric) > limit:
            return "Yellow"

    return "Green"


def spec_number_from_filename(name: str) -> str | None:
    match = SPEC_FILE_RE.match(name)
    return match.group(1) if match else None


def list_enforced_spec_files(spec_dir: Path, enforce_from: int) -> list[tuple[str, Path]]:
    out: list[tuple[str, Path]] = []
    if not spec_dir.is_dir():
        return out
    for path in sorted(spec_dir.glob("*.md")):
        if path.name in EXCLUDED_SPEC_FILES:
            continue
        spec_num = spec_number_from_filename(path.name)
        if spec_num is None:
            continue
        if int(spec_num) >= enforce_from:
            out.append((spec_num, path))
    return out


def section_content_nonempty(text: str, section_title: str) -> bool:
    """Return True if a level-2 heading contains section_title and body is non-empty."""
    lines = text.splitlines()
    capture = False
    body_lines: list[str] = []
    title_pattern = re.compile(rf"^##\s+.*{re.escape(section_title)}", re.IGNORECASE)

    for line in lines:
        if title_pattern.match(line):
            capture = True
            body_lines = []
            continue
        if capture:
            if re.match(r"^##\s+", line):
                break
            body_lines.append(line)

    if not capture:
        return False

    meaningful = [line.strip() for line in body_lines if line.strip()]
    if not meaningful:
        return False
    non_table = [line for line in meaningful if not re.match(r"^[\|\-\s]+$", line)]
    return bool(non_table)


def load_acceptance_ids(registry_path: Path, spec: str) -> set[str]:
    if not registry_path.is_file():
        return set()
    data = load_toml(registry_path)
    return {str(entry["id"]) for entry in data.get("cases", []) if str(entry["spec"]) == spec}


def load_acceptance_case(registry_path: Path, spec: str, case_id: str) -> dict[str, Any] | None:
    if not registry_path.is_file():
        return None
    data = load_toml(registry_path)
    for entry in data.get("cases", []):
        if str(entry["spec"]) == spec and str(entry["id"]) == case_id:
            return entry
    return None


def impl_spec_locations(root: Path, spec: str) -> tuple[list[Path], list[Path]]:
    tasks = list((root / "docs" / "tasks").glob(f"{spec}_*-implementation-spec.md"))
    done = list((root / "docs" / "done").glob(f"{spec}_*-implementation-spec.md"))
    return tasks, done


def load_base_registry(
    root: Path,
    base_ref: str | None,
    base_registry: Path | None,
    *,
    base_ref_required: bool,
) -> tuple[dict[str, Any] | None, list[str]]:
    """Return (registry dict or None, hard errors)."""
    load_errors: list[str] = []
    if base_registry is not None:
        if not base_registry.is_file():
            load_errors.append(f"base registry not found: {base_registry}")
            return None, load_errors
        return load_toml(base_registry), load_errors

    if not base_ref:
        return None, load_errors

    result = subprocess.run(
        ["git", "show", f"{base_ref}:scripts/feature-scope.toml"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return tomllib.loads(result.stdout), load_errors

    stderr = result.stderr
    missing_file_markers = (
        "does not exist in",
        "exists on disk, but not in",
        "Path 'scripts/feature-scope.toml' does not exist",
    )
    if any(marker in stderr for marker in missing_file_markers):
        return {"policy": {}, "features": []}, load_errors

    load_errors.append(
        "cannot load scripts/feature-scope.toml from base ref "
        f"`{base_ref}`: {stderr.strip()}"
    )
    if base_ref_required:
        return None, load_errors
    return None, load_errors


def feature_by_spec(features: list[Feature], spec: str) -> Feature | None:
    for feature in features:
        if feature.spec == spec:
            return feature
    return None


def validate_registry_integrity(
    features: list[Feature],
    errors: list[str],
) -> dict[str, Feature]:
    by_spec: dict[str, Feature] = {}
    required_scalar = (
        "spec",
        "status",
        "scope_class",
        "scope_revision",
        "scope_change_reason",
        "core_outcome",
        "actors",
        "state_machines",
        "persistent_aggregates",
        "external_effects",
        "process_boundaries",
    )
    for feature in features:
        spec = feature.spec
        if spec in by_spec:
            errors.append(f"duplicate feature spec entry: {spec}")
        by_spec[spec] = feature

        for key in required_scalar:
            if key not in feature.raw:
                errors.append(f"spec {spec}: missing required field `{key}`")

        status = str(feature.get("status", ""))
        if status not in VALID_STATUS:
            errors.append(f"spec {spec}: invalid status `{status}`")

        scope_class = str(feature.get("scope_class", ""))
        if scope_class not in VALID_SCOPE_CLASS:
            errors.append(f"spec {spec}: invalid scope_class `{scope_class}`")

        for metric in (
            "actors",
            "state_machines",
            "persistent_aggregates",
            "external_effects",
            "process_boundaries",
        ):
            if metric in feature.raw and int(feature.get(metric, -1)) < 0:
                errors.append(f"spec {spec}: `{metric}` must be non-negative")

        locked_ids = [str(x) for x in feature.get("locked_ac_ids") or []]
        if len(locked_ids) != len(set(locked_ids)):
            errors.append(f"spec {spec}: duplicate locked_ac_ids")

        vertical = str(feature.get("vertical_slice_ac_id", ""))
        if status in {"locked", "done"}:
            if int(feature.get("scope_revision", 0)) < 1:
                errors.append(f"spec {spec}: scope_revision must be >= 1 for status={status}")
            if not str(feature.get("scope_change_reason", "")).strip():
                errors.append(f"spec {spec}: scope_change_reason required for status={status}")
            if vertical and vertical not in locked_ids:
                errors.append(
                    f"spec {spec}: vertical_slice_ac_id `{vertical}` must be in locked_ac_ids"
                )

        for flag in RED_FLAG_FIELDS:
            if flag in feature.raw and not isinstance(feature.get(flag), bool):
                errors.append(f"spec {spec}: `{flag}` must be boolean")

    return by_spec


def validate_complexity_gate(
    feature: Feature,
    policy: dict[str, Any],
    spec_path: Path | None,
    errors: list[str],
) -> None:
    spec = feature.spec
    gate = classify_complexity(feature, policy)
    scope_class = str(feature.get("scope_class", "feature"))

    if gate == "Yellow":
        if str(feature.get("scope_review", "")) != "approved":
            errors.append(f"spec {spec}: Yellow requires scope_review=approved")
        if not str(feature.get("split_decision", "")).strip():
            errors.append(f"spec {spec}: Yellow requires non-empty split_decision")
        if spec_path is not None:
            text = spec_path.read_text(encoding="utf-8")
            if not section_content_nonempty(text, "Complexity Gate"):
                errors.append(f"spec {spec}: Yellow requires non-empty Complexity Gate section")

    if gate == "Red":
        if scope_class == "feature":
            errors.append(f"spec {spec}: Red complexity not allowed for scope_class=feature")
        else:
            if not str(feature.get("approved_exception", "")).strip():
                errors.append(
                    f"spec {spec}: Red {scope_class} requires non-empty approved_exception"
                )
            if not str(feature.get("split_decision", "")).strip():
                errors.append(f"spec {spec}: Red requires non-empty split_decision")


def validate_spec_presence_and_sections(
    root: Path,
    enforce_from: int,
    by_spec: dict[str, Feature],
    errors: list[str],
) -> dict[str, Path]:
    spec_dir = root / "docs" / "spec"
    spec_paths: dict[str, Path] = {}
    for spec_num, path in list_enforced_spec_files(spec_dir, enforce_from):
        spec_paths[spec_num] = path
        if spec_num not in by_spec:
            errors.append(
                f"spec {spec_num}: design spec {path.relative_to(root)} requires "
                f"feature entry in scripts/feature-scope.toml (enforce_from={enforce_from})"
            )
            continue
        text = path.read_text(encoding="utf-8")
        for section in REQUIRED_SPEC_SECTIONS:
            if not section_content_nonempty(text, section):
                errors.append(
                    f"spec {spec_num}: missing or empty required section `{section}` "
                    f"in {path.relative_to(root)}"
                )
    return spec_paths


def validate_acceptance_alignment(
    root: Path,
    feature: Feature,
    errors: list[str],
) -> None:
    status = str(feature.get("status", ""))
    if status not in {"locked", "done"}:
        return
    spec = feature.spec
    locked_ids = {str(x) for x in feature.get("locked_ac_ids") or []}
    acceptance_path = root / "scripts" / "spec-acceptance.toml"
    registry_ids = load_acceptance_ids(acceptance_path, spec)
    missing_in_registry = sorted(locked_ids - registry_ids)
    extra_in_registry = sorted(registry_ids - locked_ids)
    if missing_in_registry:
        errors.append(
            f"spec {spec}: locked_ac_ids missing from spec-acceptance.toml: "
            + ", ".join(missing_in_registry)
        )
    if extra_in_registry:
        errors.append(
            f"spec {spec}: spec-acceptance.toml ids not in locked_ac_ids: "
            + ", ".join(extra_in_registry)
        )


def validate_vertical_slice(
    root: Path,
    feature: Feature,
    errors: list[str],
) -> None:
    spec = feature.spec
    vertical = str(feature.get("vertical_slice_ac_id", "")).strip()
    if not vertical:
        errors.append(f"spec {spec}: vertical_slice_ac_id is required")
        return
    acceptance_path = root / "scripts" / "spec-acceptance.toml"
    case = load_acceptance_case(acceptance_path, spec, vertical)
    if case is None:
        errors.append(
            f"spec {spec}: vertical_slice_ac_id `{vertical}` not found in spec-acceptance.toml"
        )
        return
    if str(feature.get("status", "")) == "done" and bool(case.get("pending", False)):
        errors.append(
            f"spec {spec}: vertical slice AC `{vertical}` must have pending=false when status=done"
        )


def validate_impl_spec_placement(root: Path, feature: Feature, errors: list[str]) -> None:
    spec = feature.spec
    status = str(feature.get("status", ""))
    tasks, done = impl_spec_locations(root, spec)
    if status in {"draft", "locked"} and done and not tasks:
        rel = done[0].relative_to(root)
        errors.append(
            f"spec {spec}: status={status} but implementation spec only in {rel} "
            f"(expected docs/tasks/)"
        )
    if status == "done" and tasks and not done:
        rel = tasks[0].relative_to(root)
        errors.append(
            f"spec {spec}: status=done but implementation spec only in {rel} "
            f"(expected docs/done/)"
        )


def scope_field_value(feature: Feature, field: str) -> Any:
    if field == "locked_ac_ids":
        return sorted(str(x) for x in feature.get("locked_ac_ids") or [])
    if field == "novel_mechanisms":
        return sorted(str(x) for x in feature.get("novel_mechanisms") or [])
    if field == "integrations":
        return sorted(str(x) for x in feature.get("integrations") or [])
    return feature.get(field)


def validate_scope_lock_against_base(
    current_features: list[Feature],
    base_data: dict[str, Any],
    errors: list[str],
) -> None:
    current_by_spec = {feature.spec: feature for feature in current_features}
    for base in parse_features(base_data):
        base_status = str(base.get("status", ""))
        if base_status not in {"locked", "done"}:
            continue
        current = current_by_spec.get(base.spec)
        if current is None:
            errors.append(
                f"spec {base.spec}: feature entry removed after scope lock (was {base_status})"
            )
            continue
        current_status = str(current.get("status", ""))
        if current_status == "draft":
            errors.append(
                f"spec {base.spec}: cannot downgrade status from {base_status} to draft"
            )


def validate_scope_revisions(
    current_features: list[Feature],
    base_data: dict[str, Any] | None,
    root: Path,
    spec_paths: dict[str, Path],
    errors: list[str],
    skipped_note: list[str],
) -> None:
    if base_data is None:
        skipped_note.append("scope revision comparison skipped (no base registry)")
        return

    base_features = parse_features(base_data)
    base_by_spec = {f.spec: f for f in base_features}

    for feature in current_features:
        status = str(feature.get("status", ""))
        if status not in {"locked", "done"}:
            continue
        base = base_by_spec.get(feature.spec)
        if base is None:
            continue

        changed_fields = []
        for field in SCOPE_WATCH_FIELDS:
            if scope_field_value(feature, field) != scope_field_value(base, field):
                changed_fields.append(field)
        if not changed_fields:
            continue

        old_rev = int(base.get("scope_revision", 0))
        new_rev = int(feature.get("scope_revision", 0))
        if new_rev <= old_rev:
            errors.append(
                f"spec {feature.spec}: scope changed ({', '.join(changed_fields)}) "
                f"but scope_revision not increased ({old_rev} -> {new_rev})"
            )

        old_reason = str(base.get("scope_change_reason", "")).strip()
        new_reason = str(feature.get("scope_change_reason", "")).strip()
        if not new_reason or new_reason == old_reason:
            errors.append(
                f"spec {feature.spec}: scope changed but scope_change_reason "
                f"must differ from base and be non-empty"
            )

        spec_path = spec_paths.get(feature.spec)
        if spec_path is not None:
            text = spec_path.read_text(encoding="utf-8")
            if str(new_rev) not in text:
                errors.append(
                    f"spec {feature.spec}: Scope change log must mention revision {new_rev}"
                )


def check_template_sections(root: Path, errors: list[str]) -> None:
    template = root / "docs" / "spec" / "_feature-spec-template.md"
    if not template.is_file():
        errors.append("missing docs/spec/_feature-spec-template.md")
        return
    text = template.read_text(encoding="utf-8")
    for section in REQUIRED_SPEC_SECTIONS:
        if not section_content_nonempty(text, section):
            errors.append(f"template missing required section `{section}`")


def run_checks(
    root: Path,
    base_ref: str | None,
    base_registry: Path | None,
    *,
    check_template: bool = True,
) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    notes: list[str] = []

    registry_path = root / "scripts" / "feature-scope.toml"
    if not registry_path.is_file():
        errors.append(f"missing {registry_path.relative_to(root)}")
        return errors, notes

    data = load_toml(registry_path)
    policy = data.get("policy", {})
    enforce_from = int(policy.get("enforce_from_spec", 56))
    features = parse_features(data)
    by_spec = validate_registry_integrity(features, errors)

    spec_paths = validate_spec_presence_and_sections(root, enforce_from, by_spec, errors)

    for feature in features:
        path = spec_paths.get(feature.spec)
        validate_complexity_gate(feature, policy, path, errors)
        validate_acceptance_alignment(root, feature, errors)
        validate_vertical_slice(root, feature, errors)
        validate_impl_spec_placement(root, feature, errors)

    env_base = os.environ.get("FEATURE_SCOPE_BASE_REF", "").strip() or None
    explicit_base = bool(base_ref) or bool(base_registry) or bool(env_base)
    effective_base_ref = base_ref or env_base
    base_data, base_load_errors = load_base_registry(
        root,
        effective_base_ref,
        base_registry,
        base_ref_required=explicit_base and base_registry is None,
    )
    errors.extend(base_load_errors)
    if base_data is not None:
        validate_scope_lock_against_base(features, base_data, errors)
        validate_scope_revisions(features, base_data, root, spec_paths, errors, notes)
    elif not base_load_errors:
        notes.append("scope revision comparison skipped (no base registry)")

    if check_template:
        check_template_sections(root, errors)

    return errors, notes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="Repository root (default: parent of scripts/)",
    )
    parser.add_argument(
        "--base-ref",
        help="Git ref for scope revision comparison (e.g. origin/main)",
    )
    parser.add_argument(
        "--base-registry",
        type=Path,
        help="Path to base feature-scope.toml for revision comparison (fixtures)",
    )
    parser.add_argument(
        "--skip-template",
        action="store_true",
        help="Skip _feature-spec-template.md section checks (fixture repos)",
    )
    args = parser.parse_args()
    root = args.root.resolve()

    errors, notes = run_checks(
        root,
        args.base_ref,
        args.base_registry,
        check_template=not args.skip_template,
    )

    for note in notes:
        print(f"FEATURE SCOPE NOTE: {note}")

    if errors:
        for err in errors:
            print(f"FEATURE SCOPE FAIL: {err}", file=sys.stderr)
        print(f"FEATURE SCOPE: {len(errors)} check(s) failed", file=sys.stderr)
        return 1

    registry_path = root / "scripts" / "feature-scope.toml"
    data = load_toml(registry_path)
    policy = data.get("policy", {})
    features = parse_features(data)
    for feature in features:
        gate = classify_complexity(feature, policy)
        print(f"FEATURE SCOPE: spec {feature.spec} gate={gate}")

    print(f"FEATURE SCOPE: all checks passed ({len(features)} feature(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
