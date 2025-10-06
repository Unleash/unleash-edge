# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
