# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.1.9](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.0...unleash-edge-metrics-v20.1.9) - 2026-02-20

### ⚙️ Miscellaneous Tasks
- updated rand and serde_qs ([#1437](https://github.com/unleash/unleash-edge/issues/1437)) (by @chriswk) - #1437

### Contributors

* @chriswk

## [20.1.8](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.0...unleash-edge-metrics-v20.1.8) - 2026-02-16

### 💼 Other
- added client_id to prometheus exporting ([#1397](https://github.com/unleash/unleash-edge/issues/1397)) (by @chriswk) - #1397

### Contributors

* @chriswk

## [20.1.7](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.6...unleash-edge-metrics-v20.1.7) - 2026-01-27

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.6](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.5...unleash-edge-metrics-v20.1.6) - 2025-11-27

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.5](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.4...unleash-edge-metrics-v20.1.5) - 2025-11-20

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.4](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.3...unleash-edge-metrics-v20.1.4) - 2025-11-13

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.3](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.2...unleash-edge-metrics-v20.1.3) - 2025-11-06

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.2](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.1...unleash-edge-metrics-v20.1.2) - 2025-11-05

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.1](https://github.com/Unleash/unleash-edge/compare/unleash-edge-metrics-v20.1.0...unleash-edge-metrics-v20.1.1) - 2025-10-31

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.0.0](https://github.com/Unleash/unleash-edge/releases/tag/unleash-edge-metrics-v20.0.0) - 2025-10-06

### 🚀 Features
- impact metrics histogram ([#1187](https://github.com/unleash/unleash-edge/issues/1187)) (by @kwasniew) - #1187
- [**breaking**] Migrate off Actix to Axum ([#1109](https://github.com/unleash/unleash-edge/issues/1109)) (by @chriswk) - #1109

### 🐛 Bug Fixes
- *(cargo)* allow publish of subcrates of binary crate (by @chriswk)
- *(cargo)* move dependency declarations for sub-crates into workspace (by @chriswk)
- *(version)* Add version to each crate (by @chriswk)
- metrics send now uses appropriate token for environment ([#1197](https://github.com/unleash/unleash-edge/issues/1197)) (by @sighphyre) - #1197
- now gets first token from startup tokens for use with metrics posting ([#1196](https://github.com/unleash/unleash-edge/issues/1196)) (by @chriswk) - #1196
- smooth out metrics load by using a bounded stream rather than concurrent loop ([#1190](https://github.com/unleash/unleash-edge/issues/1190)) (by @sighphyre) - #1190
- *(metrics)* response size metric used wrong path variable (by @chriswk)
- readd observability data endpoint ([#1182](https://github.com/unleash/unleash-edge/issues/1182)) (by @chriswk) - #1182
- *(metrics)* Add error message from 403/401 response from unleash to logs ([#1177](https://github.com/unleash/unleash-edge/issues/1177)) (by @chriswk)
- *(publish)* set publish false for everything but unleash-edge crate ([#1151](https://github.com/unleash/unleash-edge/issues/1151)) (by @chriswk)

### 💼 Other
- Revert "fix(version): Add version to each crate" (by @chriswk)

### ⚙️ Miscellaneous Tasks
- release v20.0.0 ([#1143](https://github.com/unleash/unleash-edge/issues/1143)) (by @unleash-bot[bot]) - #1143

### Contributors

* @chriswk
* @unleash-bot[bot]
* @sighphyre
* @kwasniew
