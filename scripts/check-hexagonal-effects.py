#!/usr/bin/env python3
"""Hexagonal effect boundary checker — rules in hexagonal-rules.toml."""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]


@dataclass(frozen=True)
class Rule:
    id: str
    severity: str
    paths: list[str]
    regex: re.Pattern[str]
    message: str
    suggestion: str


@dataclass(frozen=True)
class AllowEntry:
    rule: str
    path: str
    line: int
    reason: str
    remove_by: str


@dataclass(frozen=True)
class Violation:
    rule_id: str
    severity: str
    path: str
    line: int
    text: str
    message: str
    suggestion: str


def load_rules(path: Path) -> list[Rule]:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    raw_rules = data.get("rules")
    if not raw_rules:
        raise ValueError(f"{path}: no [[rules]] entries")

    rules: list[Rule] = []
    seen_ids: set[str] = set()
    for idx, entry in enumerate(raw_rules):
        rule_id = entry.get("id", "")
        if not rule_id:
            raise ValueError(f"{path}: rules[{idx}] missing id")
        if rule_id in seen_ids:
            raise ValueError(f"{path}: duplicate rule id {rule_id!r}")
        seen_ids.add(rule_id)

        severity = entry.get("severity", "")
        if severity not in {"fail", "warn"}:
            raise ValueError(f"{path}: rule {rule_id!r} has invalid severity {severity!r}")

        paths = entry.get("paths")
        if not paths:
            raise ValueError(f"{path}: rule {rule_id!r} has empty paths")

        regex_raw = entry.get("regex", "")
        if not regex_raw:
            raise ValueError(f"{path}: rule {rule_id!r} has empty regex")
        try:
            compiled = re.compile(regex_raw)
        except re.error as exc:
            raise ValueError(f"{path}: rule {rule_id!r} regex invalid: {exc}") from exc

        message = entry.get("message", "")
        suggestion = entry.get("suggestion", "")
        if not message:
            raise ValueError(f"{path}: rule {rule_id!r} has empty message")
        if not suggestion:
            raise ValueError(f"{path}: rule {rule_id!r} has empty suggestion")

        rules.append(
            Rule(
                id=rule_id,
                severity=severity,
                paths=list(paths),
                regex=compiled,
                message=message,
                suggestion=suggestion,
            )
        )
    return rules


def load_allowlist(path: Path) -> list[AllowEntry]:
    if not path.exists():
        return []
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    raw = data.get("allow", [])
    entries: list[AllowEntry] = []
    for idx, entry in enumerate(raw):
        rule = entry.get("rule", "")
        rel_path = entry.get("path", "")
        line = entry.get("line")
        reason = entry.get("reason", "")
        remove_by = entry.get("remove_by", "")
        if not rule or not rel_path:
            raise ValueError(f"{path}: allow[{idx}] missing rule or path")
        if not isinstance(line, int) or line <= 0:
            raise ValueError(f"{path}: allow[{idx}] line must be a positive integer")
        if not reason:
            raise ValueError(f"{path}: allow[{idx}] missing reason")
        if not remove_by:
            raise ValueError(f"{path}: allow[{idx}] missing remove_by")
        entries.append(
            AllowEntry(
                rule=rule,
                path=rel_path.replace("\\", "/"),
                line=line,
                reason=reason,
                remove_by=remove_by,
            )
        )
    return entries


def path_matches(rel_path: str, patterns: list[str]) -> bool:
    normalized = rel_path.replace("\\", "/")
    return any(fnmatch(normalized, pattern) for pattern in patterns)


def collect_rs_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for crate in ("aibe", "aish", "ai"):
        src = root / crate / "src"
        if src.is_dir():
            files.extend(sorted(src.rglob("*.rs")))
    return files


def is_allowed(
    allowlist: list[AllowEntry], rule_id: str, rel_path: str, line: int
) -> bool:
    normalized = rel_path.replace("\\", "/")
    return any(
        entry.rule == rule_id and entry.path == normalized and entry.line == line
        for entry in allowlist
    )


def is_comment_line(text: str) -> bool:
    stripped = text.lstrip()
    return (
        stripped.startswith("//")
        or stripped.startswith("/*")
        or stripped.startswith("*/")
        or (stripped.startswith("*") and len(stripped) > 1 and stripped[1:2].isspace())
    )


def scan_file(
    root: Path,
    file_path: Path,
    rules: list[Rule],
    allowlist: list[AllowEntry],
) -> list[Violation]:
    rel = file_path.relative_to(root).as_posix()
    applicable = [rule for rule in rules if path_matches(rel, rule.paths)]
    if not applicable:
        return []

    violations: list[Violation] = []
    lines = file_path.read_text(encoding="utf-8").splitlines()
    for line_no, text in enumerate(lines, start=1):
        if is_comment_line(text):
            continue
        for rule in applicable:
            if not rule.regex.search(text):
                continue
            if is_allowed(allowlist, rule.id, rel, line_no):
                continue
            violations.append(
                Violation(
                    rule_id=rule.id,
                    severity=rule.severity,
                    path=rel,
                    line=line_no,
                    text=text.rstrip(),
                    message=rule.message,
                    suggestion=rule.suggestion,
                )
            )
    return violations


def format_violation(v: Violation) -> str:
    label = "FAIL" if v.severity == "fail" else "WARN"
    return (
        f"HEXAGONAL {label} [{v.rule_id}]\n"
        f"  {v.path}:{v.line}\n"
        f"  {v.text}\n"
        f"\n"
        f"  {v.message}\n"
        f"  Suggestion:\n"
        f"    {v.suggestion}\n"
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Hexagonal effect boundary checker")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root (default: parent of scripts/)",
    )
    parser.add_argument(
        "--rules",
        type=Path,
        default=None,
        help="path to hexagonal-rules.toml",
    )
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=None,
        help="path to hexagonal-allowlist.toml",
    )
    args = parser.parse_args(argv)

    root = args.root.resolve()
    rules_path = (args.rules or root / "scripts" / "hexagonal-rules.toml").resolve()
    allowlist_path = (
        args.allowlist or root / "scripts" / "hexagonal-allowlist.toml"
    ).resolve()

    if not rules_path.is_file():
        print(f"HEXAGONAL FAIL: rules file not found: {rules_path}", file=sys.stderr)
        return 1

    try:
        rules = load_rules(rules_path)
        allowlist = load_allowlist(allowlist_path)
    except ValueError as exc:
        print(f"HEXAGONAL FAIL: {exc}", file=sys.stderr)
        return 1

    violations: list[Violation] = []
    for file_path in collect_rs_files(root):
        violations.extend(scan_file(root, file_path, rules, allowlist))

    violations.sort(key=lambda v: (v.severity != "fail", v.path, v.line, v.rule_id))

    fail_count = 0
    warn_count = 0
    for v in violations:
        print(format_violation(v), file=sys.stderr, end="\n")
        if v.severity == "fail":
            fail_count += 1
        else:
            warn_count += 1

    if fail_count:
        print(
            f"HEXAGONAL: {fail_count} effect boundary failure(s), {warn_count} warning(s)",
            file=sys.stderr,
        )
        return 1

    if warn_count:
        print(
            f"HEXAGONAL: {warn_count} effect boundary warning(s)",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
