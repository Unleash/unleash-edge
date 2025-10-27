# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.2.0](https://github.com/Unleash/unleash-edge/compare/unleash-edge-types-v20.1.0...unleash-edge-types-v20.2.0) - 2025-10-27

### 🚀 Features
- add enterprise self hosted type ([#1233](https://github.com/unleash/unleash-edge/issues/1233)) (by @sighphyre) - #1233
- enterprise edge heartbeat ([#1211](https://github.com/unleash/unleash-edge/issues/1211)) (by @nunogois) - #1211

### Contributors

* @sighphyre
* @nunogois

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
