#!/usr/bin/env python3
"""Fail-closed checks for the intended public Git repository tree."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
checks: list[dict[str, object]] = []


def check(name: str, passed: bool, detail: str = "") -> None:
    checks.append({"name": name, "passed": bool(passed), "detail": detail})


def git_output(*args: str) -> str:
    return subprocess.check_output(
        ["git", "-C", str(ROOT), *args], text=True, stderr=subprocess.STDOUT
    )


try:
    tracked = [Path(line) for line in git_output("ls-files").splitlines() if line]
except (FileNotFoundError, subprocess.CalledProcessError):
    tracked = []

check("Git repository exists with tracked files", bool(tracked))

required = {
    Path("README.md"),
    Path("LICENSE"),
    Path(".gitignore"),
    Path(".gitattributes"),
    Path("package.json"),
    Path("package-lock.json"),
    Path("rust-toolchain.toml"),
    Path("crates/p2p-provider/Cargo.toml"),
    Path("crates/p2p-provider/Cargo.lock"),
    Path("docs/provider.md"),
    Path(".github/workflows/verify.yml"),
}
check("required public files are tracked", required <= set(tracked))

forbidden_parts = {
    "evidence",
    "node_modules",
    "target",
    ".ci-target",
    "artifacts",
    "_working_docs",
    "thinking",
}
forbidden_names = {
    "WORKING_RULES.md",
    "GITHUB_PROJECT_UPLOAD_LAW.md",
    "prompt-to-create-web-based-mac-os-app-using-tauri.md",
}
forbidden_tracked = [
    str(path)
    for path in tracked
    if forbidden_parts.intersection(path.parts) or path.name in forbidden_names
]
check(
    "no working records, evidence, dependency cache, or build output is tracked",
    not forbidden_tracked,
    ", ".join(forbidden_tracked[:10]),
)

symlinks = [str(path) for path in tracked if (ROOT / path).is_symlink()]
check("no symlink can escape the public tree", not symlinks, ", ".join(symlinks))

large_files = [
    f"{path}:{(ROOT / path).stat().st_size}"
    for path in tracked
    if (ROOT / path).is_file() and (ROOT / path).stat().st_size > 10 * 1024 * 1024
]
check("no tracked file exceeds 10 MiB", not large_files, ", ".join(large_files))

secret_patterns = {
    "GitHub classic token": re.compile(r"gh" + r"[pousr]_[A-Za-z0-9]{20,}"),
    "GitHub fine-grained token": re.compile(r"github" + r"_pat_[A-Za-z0-9_]{20,}"),
    "AWS access key": re.compile(r"AKIA[0-9A-Z]{16}"),
    "private key block": re.compile(r"BEGIN (?:OPENSSH|RSA|EC|DSA) PRIVATE KEY"),
}
local_path_patterns = {
    "Linux home path": re.compile("/" + "home/"),
    "macOS user path": re.compile("/" + "Users/"),
    "Windows user path": re.compile(r"[A-Za-z]:\\Users\\"),
}
text_suffixes = {
    ".css",
    ".html",
    ".js",
    ".json",
    ".md",
    ".py",
    ".rs",
    ".sh",
    ".toml",
    ".ts",
    ".tsx",
    ".txt",
    ".yml",
    ".yaml",
}
secret_hits: list[str] = []
path_hits: list[str] = []
for relative in tracked:
    path = ROOT / relative
    if not path.is_file() or path.suffix.lower() not in text_suffixes:
        continue
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        continue
    for label, pattern in secret_patterns.items():
        if pattern.search(text):
            secret_hits.append(f"{relative}:{label}")
    for label, pattern in local_path_patterns.items():
        if pattern.search(text):
            path_hits.append(f"{relative}:{label}")

check("no credential signature appears in tracked text", not secret_hits, ", ".join(secret_hits))
check("no local absolute user path appears in tracked text", not path_hits, ", ".join(path_hits))

package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
check("public package license is MIT", package.get("license") == "MIT")
check(
    "Rust package licenses are MIT",
    all(
        'license = "MIT"' in (ROOT / relative).read_text(encoding="utf-8")
        for relative in [
            "src-tauri/Cargo.toml",
            "crates/p2p-domain/Cargo.toml",
            "crates/p2p-provider/Cargo.toml",
        ]
    ),
)

config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
mac_config = json.loads(
    (ROOT / "src-tauri/tauri.macos.conf.json").read_text(encoding="utf-8")
)
check("neutral application identifier", config.get("identifier") == "com.p2pdesk.desktop")
check(
    "Intel macOS bundle policy declares app and macOS 12",
    mac_config.get("bundle", {}).get("targets") == ["app"]
    and mac_config.get("bundle", {}).get("macOS", {}).get("minimumSystemVersion")
    == "12.0",
)

workflow = (ROOT / ".github/workflows/verify.yml").read_text(encoding="utf-8")
action_uses = re.findall(r"^\s*uses:\s*([^\s#]+)", workflow, flags=re.MULTILINE)
check(
    "workflow actions use immutable commit pins",
    bool(action_uses)
    and all(re.fullmatch(r"[^@\s]+@[0-9a-f]{40}", value) for value in action_uses),
    ", ".join(action_uses),
)
check(
    "workflow has least read-only contents permission",
    re.search(r"(?m)^permissions:\s*\n\s+contents:\s+read\s*$", workflow) is not None,
)
check(
    "workflow covers Linux, Windows x64, and native Intel macOS",
    all(value in workflow for value in ["ubuntu-24.04", "windows-2025", "macos-15-intel"]),
)

passed = sum(1 for item in checks if item["passed"])
result = {"passed": passed, "total": len(checks), "allPassed": passed == len(checks), "checks": checks}
print(json.dumps(result, indent=2))
raise SystemExit(0 if result["allPassed"] else 1)
