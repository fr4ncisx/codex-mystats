<div align="center">

<img src="logo.png" alt="Codex-MyStats" width="320" />

**See your Codex usage limits at a glance**

A compact Windows panel for Codex CLI's five-hour and weekly usage windows.

[![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D4?style=flat-square)](https://www.microsoft.com/windows)
[![Stack: Rust and Tauri](https://img.shields.io/badge/stack-Rust%20%2B%20Tauri-DEA584?style=flat-square)](https://v2.tauri.app/)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](LICENSE)

</div>

---

## Contents

- [Contents](#contents)
- [Features](#features)
- [Requirements](#requirements)
- [Run and build](#run-and-build)
- [How updates work](#how-updates-work)
- [Troubleshooting](#troubleshooting)
- [Security](#security)
- [Releases](#releases)
- [Feedback](#feedback)
- [Disclaimer](#disclaimer)

---

## Features

- Remaining usage and reset times for the five-hour and weekly windows.
- Independent warning thresholds, starting at 20%.
- English by default, with Spanish available in Settings.
- Adjustable interface scale, window pinning, and separate tray indicators.
- One running instance; launching it again restores the open panel.

---

## Requirements

### To run

- Windows 10 or 11 with WebView2 Runtime.
- Codex CLI `0.162.1`, discoverable on `PATH` or in its standard npm installation under `%APPDATA%`.
- Codex CLI already signed in under the same Windows account.

### To build

- Windows 10 or 11.
- Rust with the `x86_64-pc-windows-msvc` target and Microsoft C++ Build Tools with the **Desktop development with C++** workload.
- Tauri CLI `2.12.2` to build the executable.

See the [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/) for platform setup. Node.js is not required to build the app.

### To run UI tests

- Node.js 24.
- Chromium managed by Playwright. Install it once with `npm run install-browsers`.

---

## Run and build

Clone the repository and run from source:

```powershell
git clone https://github.com/fr4ncisx/codex-mystats.git
Set-Location codex-mystats
cargo run
```

Build a standalone executable without an installer:

```powershell
cargo install tauri-cli --version "=2.12.2" --locked
cargo tauri build --no-bundle
```

The executable is written to `target\release\codex-mystats.exe`.

Run the frontend UI checks with Node.js 24 and Playwright:

```powershell
npm ci
npm run install-browsers
npm test
```

`npm test` runs the checks in the console. Run `npm run test-ui` to open Playwright's interactive test runner.

These tools are only needed for frontend UI tests; they are not required to build the app.

---

## How updates work

Codex-MyStats reads limits from the compatible Codex CLI over local standard input and output. It applies limit-change events as they arrive and reconciles the values every 15 seconds while connected. If a connection is lost, the panel marks the data stale and reconnects. Event delivery has no documented maximum latency.

The app identifies the five-hour and weekly windows from their durations and uses the percentage and reset time returned by Codex. It preserves the source percentage precision and does not estimate a missing window. If Codex CLI is missing, install it and restart Codex-MyStats.

---

## Troubleshooting

| Problem | What to do |
|---|---|
| Codex CLI is not found | Add `codex.exe` to `PATH` or install the supported npm package, then restart the app. |
| Codex CLI version is unsupported | Install version `0.162.1` and restart Codex-MyStats. |
| No usage is available | Sign in through Codex CLI under the same Windows account, then restart the app. |
| Data is stale | Restore Codex CLI availability; the app reconnects when the connection returns. |
| A usage window is unavailable | Check that Codex returned that period. The app does not estimate missing data. |
| `cargo tauri` is not recognized | Install the pinned Tauri CLI command above, then reopen PowerShell. |

---

## Security

Usage is not fetched anonymously: the app relies on the existing authenticated Codex CLI session for the same Windows account. Codex CLI handles its normal communication with Codex and returns usage data to the app over local standard input and output.

Codex-MyStats does not prompt for, copy, or store passwords or tokens. Its local `settings.json` stores app preferences only. The app opens no network listener and adds no telemetry of its own. The interface uses local files and narrowly scoped Tauri permissions.

---

## Releases

Pushing an annotated version tag that matches `Cargo.toml` and `tauri.conf.json` starts the Windows release workflow. It creates a draft containing the executable and its SHA-256 checksum; a maintainer reviews and publishes the draft manually. CI checks formatting, Rust tests, and JavaScript syntax on pull requests and pushes to `main`.

---

## Feedback

Report bugs and request improvements through [GitHub Issues](https://github.com/fr4ncisx/codex-mystats/issues).

---

## Disclaimer

Codex-MyStats is an independent project. It is not affiliated with or endorsed by OpenAI.

---

<div align="center">

**Include your Windows and Codex CLI versions when reporting an issue.**

</div>
