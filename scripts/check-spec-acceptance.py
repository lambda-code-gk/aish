#!/usr/bin/env python3
"""Verify acceptance-test registry (scripts/spec-acceptance.toml) against the repo."""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "scripts" / "spec-acceptance.toml"
SPEC_INDEX = ROOT / "docs" / "0000_spec-index.md"
TASKS_DIR = ROOT / "docs" / "tasks"
DONE_DIR = ROOT / "docs" / "done"

IGNORE_RE = re.compile(r"#\s*\[\s*ignore\b", re.IGNORECASE)


def fn_pattern(name: str) -> re.Pattern[str]:
    return re.compile(rf"\bfn\s+({re.escape(name)})\s*\(")


@dataclass(frozen=True)
class Case:
    spec: str
    phase: int
    id: str
    description: str
    test: str
    file_glob: str
    pending: bool


def load_cases() -> list[Case]:
    raw = tomllib.loads(REGISTRY.read_text(encoding="utf-8"))
    out: list[Case] = []
    for entry in raw.get("cases", []):
        out.append(
            Case(
                spec=str(entry["spec"]),
                phase=int(entry["phase"]),
                id=str(entry["id"]),
                description=str(entry.get("description", "")),
                test=str(entry["test"]),
                file_glob=str(entry.get("file_glob", "**/*.rs")),
                pending=bool(entry.get("pending", False)),
            )
        )
    return out


def rs_files() -> list[Path]:
    return [p for p in ROOT.rglob("*.rs") if "target" not in p.parts]


def find_test_location(test_name: str, file_glob: str) -> tuple[Path | None, bool]:
    """Return (file, is_ignored)."""
    candidates = [
        p
        for p in rs_files()
        if fnmatch(str(p.relative_to(ROOT)), file_glob)
        and fn_pattern(test_name).search(p.read_text(encoding="utf-8"))
    ]
    if not candidates:
        return None, False
    path = candidates[0]
    text = path.read_text(encoding="utf-8")
    ignored = False
    pattern = fn_pattern(test_name)
    for match in pattern.finditer(text):
        prefix = text[: match.start()]
        # Look at the few lines immediately above the fn for #[ignore].
        tail = prefix.splitlines()[-6:]
        if any(IGNORE_RE.search(line) for line in tail):
            ignored = True
        break
    return path, ignored


def specs_with_pending(cases: list[Case]) -> set[str]:
    return {c.spec for c in cases if c.pending}


def check_impl_spec_placement(pending_specs: set[str]) -> list[str]:
    errors: list[str] = []
    for spec in sorted(pending_specs):
        done_matches = list(DONE_DIR.glob(f"{spec}_*-implementation-spec.md"))
        task_matches = list(TASKS_DIR.glob(f"{spec}_*-implementation-spec.md"))
        if done_matches and not task_matches:
            rel = done_matches[0].relative_to(ROOT)
            errors.append(
                f"spec {spec} has pending acceptance cases but implementation spec is in {rel} "
                f"(move to docs/tasks/ until all cases are pending=false)"
            )
    return errors


def check_index_not_claims_done(pending_specs: set[str]) -> list[str]:
    errors: list[str] = []
    if not SPEC_INDEX.is_file():
        return ["missing docs/0000_spec-index.md"]
    text = SPEC_INDEX.read_text(encoding="utf-8")
    for spec in sorted(pending_specs):
        # Design spec row: avoid "設計確定（実装済み）" for in-flight specs.
        if re.search(
            rf"\| {spec} \|.*設計確定（実装済み）",
            text,
        ):
            errors.append(
                f"docs/0000_spec-index.md still marks spec {spec} design as 実装済み "
                f"while acceptance cases are pending"
            )
        # Master implementation spec in done/ must not claim full 実装済み while any AC pending.
        # Phase-specific rows (…-phaseN-implementation-spec.md) may stay 実装済み（Phase N）.
        for line in text.splitlines():
            if not line.startswith(f"| {spec} |"):
                continue
            if "implementation-spec.md](done/" not in line or "| 実装済み" not in line:
                continue
            if re.search(r"-phase[0-9]", line):
                continue
            errors.append(
                f"docs/0000_spec-index.md lists spec {spec} implementation as 実装済み in done/ "
                f"while acceptance cases are pending"
            )
            break
    return errors


def main() -> int:
    if not REGISTRY.is_file():
        print(f"ACCEPTANCE FAIL: missing {REGISTRY}", file=sys.stderr)
        return 1

    cases = load_cases()
    errors: list[str] = []
    warnings: list[str] = []

    for case in cases:
        path, ignored = find_test_location(case.test, case.file_glob)
        if path is None:
            errors.append(
                f"{case.spec} phase {case.phase} {case.id}: "
                f"missing test fn `{case.test}` (glob={case.file_glob})"
            )
            continue
        if case.pending:
            if not ignored:
                warnings.append(
                    f"{case.spec} phase {case.phase} {case.id}: "
                    f"`{case.test}` is pending but not #[ignore] — "
                    f"will fail CI until implementation lands"
                )
        elif ignored:
            errors.append(
                f"{case.spec} phase {case.phase} {case.id}: "
                f"`{case.test}` must not be #[ignore] (pending=false)"
            )

    pending_specs = specs_with_pending(cases)
    errors.extend(check_impl_spec_placement(pending_specs))
    errors.extend(check_index_not_claims_done(pending_specs))

    for warning in warnings:
        print(f"ACCEPTANCE WARN: {warning}", file=sys.stderr)

    if errors:
        for err in errors:
            print(f"ACCEPTANCE FAIL: {err}", file=sys.stderr)
        print(f"ACCEPTANCE: {len(errors)} check(s) failed", file=sys.stderr)
        return 1

    print(
        f"ACCEPTANCE: all {len(cases)} case(s) ok "
        f"({len(pending_specs)} spec(s) with pending cases)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
