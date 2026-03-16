# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.1.11](https://github.com/Unleash/unleash-edge/compare/unleash-edge-feature-filters-v20.1.10...unleash-edge-feature-filters-v20.1.11) - 2026-03-16

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.10](https://github.com/Unleash/unleash-edge/compare/unleash-edge-feature-filters-v20.1.9...unleash-edge-feature-filters-v20.1.10) - 2026-02-24

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.9](https://github.com/Unleash/unleash-edge/compare/unleash-edge-feature-filters-v20.1.8...unleash-edge-feature-filters-v20.1.9) - 2026-02-23

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.1.8](https://github.com/Unleash/unleash-edge/compare/unleash-edge-feature-filters-v20.1.1...unleash-edge-feature-filters-v20.1.8) - 2026-02-16

### 🐛 Bug Fixes
- make clippy work again on CI ([#1432](https://github.com/unleash/unleash-edge/issues/1432)) (by @sighphyre) - #1432
- filter out unnecessary segments when responding to features requests ([#1400](https://github.com/unleash/unleash-edge/issues/1400)) (by @sighphyre) - #1400

### Contributors

* @sighphyre

## [20.1.7](https://github.com/Unleash/unleash-edge/compare/unleash-edge-feature-filters-v20.1.6...unleash-edge-feature-filters-v20.1.7) - 2026-01-27

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.0.0](https://github.com/Unleash/unleash-edge/releases/tag/unleash-edge-feature-filters-v20.0.0) - 2025-10-06

### 🚀 Features
- [**breaking**] Migrate off Actix to Axum ([#1109](https://github.com/unleash/unleash-edge/issues/1109)) (by @chriswk) - #1109

### 🐛 Bug Fixes
- *(cargo)* allow publish of subcrates of binary crate (by @chriswk)
- *(cargo)* move dependency declarations for sub-crates into workspace (by @chriswk)
- *(version)* Add version to each crate (by @chriswk)
- *(publish)* set publish false for everything but unleash-edge crate ([#1151](https://github.com/unleash/unleash-edge/issues/1151)) (by @chriswk)

### 💼 Other
- Revert "fix(version): Add version to each crate" (by @chriswk)

### Contributors

* @chriswk
