# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.1.8](https://github.com/Unleash/unleash-edge/compare/unleash-edge-client-api-v20.1.0...unleash-edge-client-api-v20.1.8) - 2026-02-16

### 🚀 Features
- resume streaming from Last-Event-ID to avoid hydration on reconnect ([#1436](https://github.com/unleash/unleash-edge/issues/1436)) (by @gastonfournier) - #1436

### Contributors

* @gastonfournier

## [20.1.7](https://github.com/Unleash/unleash-edge/compare/unleash-edge-client-api-v20.1.6...unleash-edge-client-api-v20.1.7) - 2026-01-27

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.6](https://github.com/Unleash/unleash-edge/compare/unleash-edge-client-api-v20.1.5...unleash-edge-client-api-v20.1.6) - 2025-11-27

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.5](https://github.com/Unleash/unleash-edge/compare/unleash-edge-client-api-v20.1.4...unleash-edge-client-api-v20.1.5) - 2025-11-20

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.3](https://github.com/Unleash/unleash-edge/compare/unleash-edge-client-api-v20.1.2...unleash-edge-client-api-v20.1.3) - 2025-11-06

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.1](https://github.com/Unleash/unleash-edge/compare/unleash-edge-client-api-v20.1.0...unleash-edge-client-api-v20.1.1) - 2025-10-31

### ⚙️ Miscellaneous Tasks
- move enterprise features to their own folder ([#1254](https://github.com/unleash/unleash-edge/issues/1254)) (by @sighphyre) - #1254

### Contributors

* @sighphyre

## [20.0.0](https://github.com/Unleash/unleash-edge/releases/tag/unleash-edge-client-api-v20.0.0) - 2025-10-06

### 🚀 Features
- impact metrics histogram ([#1187](https://github.com/unleash/unleash-edge/issues/1187)) (by @kwasniew) - #1187
- [**breaking**] Migrate off Actix to Axum ([#1109](https://github.com/unleash/unleash-edge/issues/1109)) (by @chriswk) - #1109

### 🐛 Bug Fixes
- *(cargo)* allow publish of subcrates of binary crate (by @chriswk)
- *(cargo)* move dependency declarations for sub-crates into workspace (by @chriswk)
- *(version)* Add version to each crate (by @chriswk)
- readd observability data endpoint ([#1182](https://github.com/unleash/unleash-edge/issues/1182)) (by @chriswk) - #1182
- *(publish)* set publish false for everything but unleash-edge crate ([#1151](https://github.com/unleash/unleash-edge/issues/1151)) (by @chriswk)

### 💼 Other
- Revert "fix(version): Add version to each crate" (by @chriswk)

### ⚙️ Miscellaneous Tasks
- release v20.0.0 ([#1143](https://github.com/unleash/unleash-edge/issues/1143)) (by @unleash-bot[bot]) - #1143
- add trace logging to validator middleware to help debug geotab… ([#1192](https://github.com/unleash/unleash-edge/issues/1192)) (by @chriswk) - #1192

### Contributors

* @chriswk
* @unleash-bot[bot]
* @kwasniew
