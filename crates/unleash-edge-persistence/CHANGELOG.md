# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.2.0](https://github.com/Unleash/unleash-edge/compare/unleash-edge-persistence-v20.1.0...unleash-edge-persistence-v20.2.0) - 2025-10-27

### 🚀 Features
- add license state to backup ([#1215](https://github.com/unleash/unleash-edge/issues/1215)) (by @nunogois) - #1215

### ⚙️ Miscellaneous Tasks
- isolate file io tests to be independent of each other ([#1242](https://github.com/unleash/unleash-edge/issues/1242)) (by @sighphyre) - #1242
- restore persistence tests ([#1220](https://github.com/unleash/unleash-edge/issues/1220)) (by @sighphyre) - #1220

### Contributors

* @sighphyre
* @nunogois

## [20.1.0](https://github.com/Unleash/unleash-edge/compare/unleash-edge-persistence-v20.0.0...unleash-edge-persistence-v20.1.0) - 2025-10-10

### ⚙️ Miscellaneous Tasks
- update Cargo.toml dependencies

## [20.0.0](https://github.com/Unleash/unleash-edge/releases/tag/unleash-edge-persistence-v20.0.0) - 2025-10-06

### 🚀 Features
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
- better logging for s3 ([#1180](https://github.com/unleash/unleash-edge/issues/1180)) (by @sighphyre) - #1180

### Contributors

* @chriswk
* @unleash-bot[bot]
* @sighphyre
