#!/usr/bin/env python3
"""Gate 1 deterministic least-privilege and local-only audit."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
checks: list[dict[str, object]] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    checks.append({"name": name, "passed": condition, "detail": detail})


config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
security = config["app"]["security"]
production_csp: str = security["csp"]
dev_csp: str = security["devCsp"]
window = config["app"]["windows"][0]
capability = json.loads((ROOT / "src-tauri/capabilities/main.json").read_text())
root_package = json.loads((ROOT / "package.json").read_text())
app_package = json.loads((ROOT / "app/package.json").read_text())
cargo = (ROOT / "src-tauri/Cargo.toml").read_text()
build_rs = (ROOT / "src-tauri/build.rs").read_text()
vite_config = (ROOT / "app/vite.config.ts").read_text()
rust_sources = "\n".join(p.read_text() for p in (ROOT / "src-tauri/src").glob("*.rs"))
web_sources = "\n".join(
    p.read_text()
    for p in (ROOT / "app/src").rglob("*")
    if p.is_file() and p.suffix in {".ts", ".tsx", ".css"}
)

check("production CSP has no unsafe/eval", "unsafe-inline" not in production_csp and "unsafe-eval" not in production_csp, production_csp)
check("production CSP allows only Tauri IPC connections", "connect-src ipc: http://ipc.localhost" in production_csp and "https:" not in production_csp and "ws:" not in production_csp, production_csp)
check("production CSP blocks objects/frames/forms/base changes", all(x in production_csp for x in ["object-src 'none'", "frame-src 'none'", "frame-ancestors 'none'", "form-action 'none'", "base-uri 'none'"]))
check("development CSP is localhost-only", "localhost:1420" in dev_csp and "https:" not in dev_csp)
check("global Tauri API disabled", config["app"]["withGlobalTauri"] is False)
check("prototype freezing enabled", security["freezePrototype"] is True)
check("unused Tauri commands removed", config["build"]["removeUnusedCommands"] is True)
check("single named capability", security["capabilities"] == ["main"] and capability["windows"] == ["main"])
check("single bootstrap permission", capability["permissions"] == ["allow-get-bootstrap-info"])
check("AppManifest restricts command inventory", 'commands(COMMANDS)' in build_rs and '"get_bootstrap_info"' in build_rs)
check("no shell/http/fs plugin dependency", all(token not in cargo for token in ["tauri-plugin-shell", "tauri-plugin-http", "tauri-plugin-fs"]))
check("no frontend shell/http/fs package", all(token not in json.dumps(app_package) for token in ["plugin-shell", "plugin-http", "plugin-fs"]))
check("no unsafe web sinks", not re.search(r"dangerouslySetInnerHTML|\.innerHTML\s*=|\beval\s*\(|new\s+Function|document\.write", web_sources))
check("no frontend network API", not re.search(r"\bfetch\s*\(|XMLHttpRequest|WebSocket\s*\(", web_sources))
check("no generated module-preload fetch polyfill", "modulePreload: { polyfill: false }" in vite_config)
check("no Rust process or command execution", "std::process::Command" not in rust_sources and "std::process::command" not in rust_sources.lower())
check("no Rust provider networking dependency", all(token not in cargo for token in ["reqwest", "ureq", "hyper =", "tauri-plugin-http"]))
check("system WebView2 only", config["bundle"]["windows"]["webviewInstallMode"]["type"] == "skip" and "GetAvailableCoreWebView2BrowserVersionString" in rust_sources)
check("portable no-bundle config", config["bundle"]["active"] is False and root_package["scripts"]["tauri:build"].endswith("--no-bundle"))
check("approved product identity", config["productName"] == "P2P Desk" and config["mainBinaryName"] == "P2PDesk" and window["title"] == "P2P Desk")
check("approved window policy", all([window["width"] == 1280, window["height"] == 800, window["minWidth"] == 1024, window["minHeight"] == 700, window["decorations"] is True, window["maximized"] is False, window["fullscreen"] is False, window["preventOverflow"] is True]))
check("LocalAppData abstraction and product root", ".local_data_dir()" in rust_sources and 'join(PRODUCT_NAME)' in rust_sources)
check("no floating npm versions", all(not str(v).startswith(("^", "~", ">", "<", "latest")) for section in [root_package.get("devDependencies", {}), app_package.get("dependencies", {}), app_package.get("devDependencies", {})] for v in section.values()))
check("pinned engine versions", root_package["engines"] == {"node": "24.18.1", "npm": "11.16.0"})

passed = sum(1 for item in checks if item["passed"])
report = {"passed": passed, "total": len(checks), "allPassed": passed == len(checks), "checks": checks}
print(json.dumps(report, indent=2))
sys.exit(0 if report["allPassed"] else 1)
