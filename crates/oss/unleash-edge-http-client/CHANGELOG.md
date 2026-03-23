# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.2.1](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.2.0...unleash-edge-http-client-v20.2.1) - 2026-03-23

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.10](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.1...unleash-edge-http-client-v20.1.10) - 2026-02-24

### ⚙️ Miscellaneous Tasks
- initial work on no longer using CliArgs longer than we need ([#1451](https://github.com/unleash/unleash-edge/issues/1451)) (by @chriswk) - #1451

### Contributors

* @chriswk

## [20.1.9](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.1...unleash-edge-http-client-v20.1.9) - 2026-02-23

### 🐛 Bug Fixes
- unleash expects path to be /edge/issue-token ([#1445](https://github.com/unleash/unleash-edge/issues/1445)) (by @chriswk) - #1445

### ⚙️ Miscellaneous Tasks
- updated rand and serde_qs ([#1437](https://github.com/unleash/unleash-edge/issues/1437)) (by @chriswk) - #1437

### Contributors

* @chriswk

## [20.1.8](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.1...unleash-edge-http-client-v20.1.8) - 2026-02-16

### 🚀 Features
- hmac client token acquisition ([#1424](https://github.com/unleash/unleash-edge/issues/1424)) (by @chriswk) - #1424

### Contributors

* @chriswk

## [20.1.7](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.1...unleash-edge-http-client-v20.1.7) - 2026-01-27

### 🚀 Features
- add Opentelemetry tracing support to enterprise ([#1372](https://github.com/unleash/unleash-edge/issues/1372)) (by @chriswk) - #1372

### Contributors

* @chriswk

## [20.1.6](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.5...unleash-edge-http-client-v20.1.6) - 2025-11-27

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.5](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.4...unleash-edge-http-client-v20.1.5) - 2025-11-20

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.4](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.3...unleash-edge-http-client-v20.1.4) - 2025-11-13

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.3](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.2...unleash-edge-http-client-v20.1.3) - 2025-11-06

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.2](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.1...unleash-edge-http-client-v20.1.2) - 2025-11-05

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.1](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.0...unleash-edge-http-client-v20.1.1) - 2025-10-31

### ⚙️ Miscellaneous Tasks
- move enterprise features to their own folder ([#1254](https://github.com/unleash/unleash-edge/issues/1254)) (by @sighphyre) - #1254

### Contributors

* @sighphyre

## [20.0.0](https://github.com/Unleash/unleash-edge/releases/tag/unleash-edge-http-client-v20.0.0) - 2025-10-06

### 🚀 Features
- [**breaking**] Migrate off Actix to Axum ([#1109](https://github.com/unleash/unleash-edge/issues/1109)) (by @chriswk) - #1109

### 🐛 Bug Fixes
- *(cargo)* allow publish of subcrates of binary crate (by @chriswk)
- *(cargo)* move dependency declarations for sub-crates into workspace (by @chriswk)
- *(version)* Add version to each crate (by @chriswk)
- smooth out metrics load by using a bounded stream rather than concurrent loop ([#1190](https://github.com/unleash/unleash-edge/issues/1190)) (by @sighphyre) - #1190
- *(socket)* Setup slow loris protection. ([#1188](https://github.com/unleash/unleash-edge/issues/1188)) (by @chriswk)
- *(prometheus)* incorrect metrics cardinality (by @chriswk)
- *(metrics)* Add error message from 403/401 response from unleash to logs ([#1177](https://github.com/unleash/unleash-edge/issues/1177)) (by @chriswk)
- *(publish)* set publish false for everything but unleash-edge crate ([#1151](https://github.com/unleash/unleash-edge/issues/1151)) (by @chriswk)

### 💼 Other
- Revert "fix(version): Add version to each crate" (by @chriswk)

### ⚙️ Miscellaneous Tasks
- release v20.0.0 ([#1143](https://github.com/unleash/unleash-edge/issues/1143)) (by @unleash-bot[bot]) - #1143
- redact trace level token in send metrics (by @sighphyre)

### Contributors

* @chriswk
* @unleash-bot[bot]
* @sighphyre
