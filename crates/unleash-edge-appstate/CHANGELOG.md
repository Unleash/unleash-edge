# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.0.0](https://github.com/Unleash/unleash-edge/releases/tag/unleash-edge-appstate-v20.0.0) - 2025-10-06

### 🚀 Features
- [**breaking**] Migrate off Actix to Axum ([#1109](https://github.com/unleash/unleash-edge/issues/1109)) (by @chriswk) - #1109

### 🐛 Bug Fixes
- *(cargo)* allow publish of subcrates of binary crate (by @chriswk)
- *(cargo)* move dependency declarations for sub-crates into workspace (by @chriswk)
- *(version)* Add version to each crate (by @chriswk)
- readd observability data endpoint ([#1182](https://github.com/unleash/unleash-edge/issues/1182)) (by @chriswk) - #1182
- readded hosting to EdgeInstanceData ([#1175](https://github.com/unleash/unleash-edge/issues/1175)) (by @chriswk) - #1175
- *(publish)* set publish false for everything but unleash-edge crate ([#1151](https://github.com/unleash/unleash-edge/issues/1151)) (by @chriswk)

### 💼 Other
- Revert "fix(version): Add version to each crate" (by @chriswk)

### ⚙️ Miscellaneous Tasks
- add trace logging to validator middleware to help debug geotab… ([#1192](https://github.com/unleash/unleash-edge/issues/1192)) (by @chriswk) - #1192
- add token status gauges ([#1191](https://github.com/unleash/unleash-edge/issues/1191)) (by @chriswk) - #1191

### Contributors

* @chriswk
