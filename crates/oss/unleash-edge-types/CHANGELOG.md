# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.1.7](https://github.com/Unleash/unleash-edge/compare/unleash-edge-types-v20.1.1...unleash-edge-types-v20.1.7) - 2026-01-27

### 🚀 Features
- add revisionId to things updated by refreshers ([#1378](https://github.com/unleash/unleash-edge/issues/1378)) (by @chriswk) - #1378
- add Opentelemetry tracing support to enterprise ([#1372](https://github.com/unleash/unleash-edge/issues/1372)) (by @chriswk) - #1372

### Contributors

* @chriswk

## [20.1.6](https://github.com/Unleash/unleash-edge/compare/unleash-edge-types-v20.1.5...unleash-edge-types-v20.1.6) - 2025-11-27

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.5](https://github.com/Unleash/unleash-edge/compare/unleash-edge-types-v20.1.4...unleash-edge-types-v20.1.5) - 2025-11-20

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.2](https://github.com/Unleash/unleash-edge/compare/unleash-edge-types-v20.1.1...unleash-edge-types-v20.1.2) - 2025-11-05

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.1](https://github.com/Unleash/unleash-edge/compare/unleash-edge-types-v20.1.0...unleash-edge-types-v20.1.1) - 2025-10-31

### ⚙️ Miscellaneous Tasks
- move enterprise features to their own folder ([#1254](https://github.com/unleash/unleash-edge/issues/1254)) (by @sighphyre) - #1254

### Contributors

* @sighphyre

## [20.0.0](https://github.com/Unleash/unleash-edge/releases/tag/unleash-edge-types-v20.0.0) - 2025-10-06

### 🚀 Features
- [**breaking**] Migrate off Actix to Axum ([#1109](https://github.com/unleash/unleash-edge/issues/1109)) (by @chriswk) - #1109

### 🐛 Bug Fixes
- *(cargo)* allow publish of subcrates of binary crate (by @chriswk)
- *(version)* Add version to each crate (by @chriswk)
- env variable is EDGE_HOSTING (by @chriswk)
- smooth out metrics load by using a bounded stream rather than concurrent loop ([#1190](https://github.com/unleash/unleash-edge/issues/1190)) (by @sighphyre) - #1190
- readd observability data endpoint ([#1182](https://github.com/unleash/unleash-edge/issues/1182)) (by @chriswk) - #1182
- readded hosting to EdgeInstanceData ([#1175](https://github.com/unleash/unleash-edge/issues/1175)) (by @chriswk) - #1175
- *(publish)* set publish false for everything but unleash-edge crate ([#1151](https://github.com/unleash/unleash-edge/issues/1151)) (by @chriswk)

### 💼 Other
- Revert "fix(version): Add version to each crate" (by @chriswk)

### ⚙️ Miscellaneous Tasks
- release v20.0.0 ([#1143](https://github.com/unleash/unleash-edge/issues/1143)) (by @unleash-bot[bot]) - #1143

### Contributors

* @chriswk
* @unleash-bot[bot]
* @sighphyre
