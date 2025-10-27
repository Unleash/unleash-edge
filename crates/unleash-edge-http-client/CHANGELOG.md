# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [20.2.0](https://github.com/Unleash/unleash-edge/compare/unleash-edge-http-client-v20.1.0...unleash-edge-http-client-v20.2.0) - 2025-10-27

### 🚀 Features
- enterprise edge heartbeat ([#1211](https://github.com/unleash/unleash-edge/issues/1211)) (by @nunogois) - #1211

### 🚜 Refactor
- test config ([#1229](https://github.com/unleash/unleash-edge/issues/1229)) (by @sighphyre) - #1229

### ⚙️ Miscellaneous Tasks
- isolate app state ([#1230](https://github.com/unleash/unleash-edge/issues/1230)) (by @sighphyre) - #1230

### Contributors

* @sighphyre
* @nunogois

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
