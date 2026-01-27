# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.1.7](https://github.com/Unleash/unleash-edge/compare/unleash-edge-cli-v20.1.1...unleash-edge-cli-v20.1.7) - 2026-01-27

### 🚀 Features
- add Opentelemetry tracing support to enterprise ([#1372](https://github.com/unleash/unleash-edge/issues/1372)) (by @chriswk) - #1372

### Contributors

* @chriswk

## [20.1.6](https://github.com/Unleash/unleash-edge/compare/unleash-edge-cli-v20.1.5...unleash-edge-cli-v20.1.6) - 2025-11-27

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.5](https://github.com/Unleash/unleash-edge/compare/unleash-edge-cli-v20.1.4...unleash-edge-cli-v20.1.5) - 2025-11-20

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.1](https://github.com/Unleash/unleash-edge/compare/unleash-edge-cli-v20.1.0...unleash-edge-cli-v20.1.1) - 2025-10-31

### ⚙️ Miscellaneous Tasks
- feature gate cli args ([#1256](https://github.com/unleash/unleash-edge/issues/1256)) (by @sighphyre) - #1256
- move enterprise features to their own folder ([#1254](https://github.com/unleash/unleash-edge/issues/1254)) (by @sighphyre) - #1254

### Contributors

* @sighphyre

## [20.1.0](https://github.com/Unleash/unleash-edge/compare/unleash-edge-cli-v20.0.0...unleash-edge-cli-v20.1.0) - 2025-10-10

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.0.0](https://github.com/Unleash/unleash-edge/releases/tag/unleash-edge-cli-v20.0.0) - 2025-10-06

### 🚀 Features
- allow explicit * in CORS origin ([#1174](https://github.com/unleash/unleash-edge/issues/1174)) (by @nunogois) - #1174
- [**breaking**] Migrate off Actix to Axum ([#1109](https://github.com/unleash/unleash-edge/issues/1109)) (by @chriswk) - #1109

### 🐛 Bug Fixes
- *(cargo)* allow publish of subcrates of binary crate (by @chriswk)
- *(cargo)* move dependency declarations for sub-crates into workspace (by @chriswk)
- *(version)* Add version to each crate (by @chriswk)
- *(publish)* set publish false for everything but unleash-edge crate ([#1151](https://github.com/unleash/unleash-edge/issues/1151)) (by @chriswk)

### 💼 Other
- Revert "fix(version): Add version to each crate" (by @chriswk)

### ⚙️ Miscellaneous Tasks
- release v20.0.0 ([#1143](https://github.com/unleash/unleash-edge/issues/1143)) (by @unleash-bot[bot]) - #1143

### Contributors

* @chriswk
* @unleash-bot[bot]
* @nunogois
