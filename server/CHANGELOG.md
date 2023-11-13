# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 16.0.3 (2023-11-13)

### Bug Fixes

 - <csr-id-7a9c453e35ba1f93ffbd5f42b969cd79cc98b873/> upgrades yggdrasil to 0.8.0, this fixes an issue with constraints not parsing correctly with free quotes

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 1 commit contributed to the release.
 - 4 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#337](https://github.com/Unleash/unleash-edge/issues/337)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#337](https://github.com/Unleash/unleash-edge/issues/337)**
    - upgrades yggdrasil to 0.8.0, this fixes an issue with constraints not parsing correctly with free quotes ([`7a9c453`](https://github.com/Unleash/unleash-edge/commit/7a9c453e35ba1f93ffbd5f42b969cd79cc98b873))
</details>

## 16.0.2 (2023-11-09)

### Bug Fixes

 - <csr-id-48e6498171e3f805a643a3ad3200f7be2cb37ce3/> Wildcard token refreshing.
   * fix: Wildcard token refreshing.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 7 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#330](https://github.com/Unleash/unleash-edge/issues/330)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#330](https://github.com/Unleash/unleash-edge/issues/330)**
    - Wildcard token refreshing. ([`48e6498`](https://github.com/Unleash/unleash-edge/commit/48e6498171e3f805a643a3ad3200f7be2cb37ce3))
 * **Uncategorized**
    - Release unleash-edge v16.0.2 ([`0b1380a`](https://github.com/Unleash/unleash-edge/commit/0b1380ada39c65ec181761474338004eaadfd879))
</details>

<csr-unknown>
Previously our refreshing algorithm assumed that we always had at leastone project with an explicit name for deciding what to keep. When inreality we could have a wildcard token (*) for updating. This means thatwe should just return the update as our new set of data for this token.Added another test with a wildcard token to verify that we do indeedonly keep the update.In addition, added a filter for unique feature names to what we’rekeeping to avoid mixing multiple features of the same name.<csr-unknown/>

## 16.0.1 (2023-11-01)

<csr-id-fc5ded0a1398e21d7fe17c1277fcf6af4f5d15e1/>

### Chore

 - <csr-id-fc5ded0a1398e21d7fe17c1277fcf6af4f5d15e1/> prepare for 16.0.1 release

### Bug Fixes

 - <csr-id-5ddd1f53124a55d65bff97f30589cd810bedaaf6/> Handle archived/deleted projects
   Previously, our cache refresh algorithm assumed that the response from
   upstream contained all projects we wanted to do updates to. Wayfair
   correctly reported this breaking their opportunity to archive/delete
   projects, since the cache would still contain deleted projects.
   
   This patch updates Edge to use the projects the token has access to
   decide whether or not to keep the elements in cache.
   
   New flow:
   1. Fetch projects from token
2. Filter out all features belonging to these projects
3. Extend remaining list with update from response
4. Return extended list.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#324](https://github.com/Unleash/unleash-edge/issues/324)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#324](https://github.com/Unleash/unleash-edge/issues/324)**
    - Handle archived/deleted projects ([`5ddd1f5`](https://github.com/Unleash/unleash-edge/commit/5ddd1f53124a55d65bff97f30589cd810bedaaf6))
 * **Uncategorized**
    - Release unleash-edge v16.0.1 ([`f3c4c62`](https://github.com/Unleash/unleash-edge/commit/f3c4c623138de6fbe1868099098c58526b1587d6))
    - prepare for 16.0.1 release ([`fc5ded0`](https://github.com/Unleash/unleash-edge/commit/fc5ded0a1398e21d7fe17c1277fcf6af4f5d15e1))
</details>

## 16.0.0 (2023-11-01)

<csr-id-fbd72a8bc8b64b388fc4fe0fc1de61bf5ff59b7f/>

### Chore

 - <csr-id-fbd72a8bc8b64b388fc4fe0fc1de61bf5ff59b7f/> prepare for release

### New Features

 - <csr-id-d27b81d2cdb7a7f7ea049a7e96c7b79bdabdbfe5/> Add support for setting log format at startup

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 1 day passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#322](https://github.com/Unleash/unleash-edge/issues/322)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#322](https://github.com/Unleash/unleash-edge/issues/322)**
    - Add support for setting log format at startup ([`d27b81d`](https://github.com/Unleash/unleash-edge/commit/d27b81d2cdb7a7f7ea049a7e96c7b79bdabdbfe5))
 * **Uncategorized**
    - Release unleash-edge v16.0.0 ([`fe761a2`](https://github.com/Unleash/unleash-edge/commit/fe761a2e10ab2f7ddba50d40afbd06923abc4b39))
    - prepare for release ([`fbd72a8`](https://github.com/Unleash/unleash-edge/commit/fbd72a8bc8b64b388fc4fe0fc1de61bf5ff59b7f))
</details>

## 15.0.0 (2023-10-30)

<csr-id-3f94d5bf593daa34e671e972789a213206eea92e/>
<csr-id-0bda1cfd8fc157c36a3486acc2d949b2fccc15e4/>
<csr-id-0c6b33a1e011b76fd75618e4ba3cb8a52f7e6c2c/>
<csr-id-1b4277e211e8ece600b482fe49544163bcbf5eb9/>
<csr-id-30958a1cddb86c2abd705b2d4fd36b51037f6879/>

### Chore

 - <csr-id-3f94d5bf593daa34e671e972789a213206eea92e/> bump yggdrasil version
 - <csr-id-0bda1cfd8fc157c36a3486acc2d949b2fccc15e4/> remove dotenv and bump ahash to a non-yanked version
 - <csr-id-0c6b33a1e011b76fd75618e4ba3cb8a52f7e6c2c/> dependencies bump
 - <csr-id-1b4277e211e8ece600b482fe49544163bcbf5eb9/> bump renovate bot suggestions
 - <csr-id-30958a1cddb86c2abd705b2d4fd36b51037f6879/> Start work upgrading to opentelemetry 0.20

### Documentation

 - <csr-id-8279ec0c411586c91b1f7d5214be963a47bff9e0/> Update dependency pointer to 14.0.0

### New Features

 - <csr-id-e77ea1846ae01e9489c048db64c87e53ebdb6fb0/> make edge log bad request information from upstream

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 9 commits contributed to the release over the course of 14 calendar days.
 - 14 days passed between releases.
 - 7 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 3 unique issues were worked on: [#292](https://github.com/Unleash/unleash-edge/issues/292), [#313](https://github.com/Unleash/unleash-edge/issues/313), [#319](https://github.com/Unleash/unleash-edge/issues/319)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#292](https://github.com/Unleash/unleash-edge/issues/292)**
    - Start work upgrading to opentelemetry 0.20 ([`30958a1`](https://github.com/Unleash/unleash-edge/commit/30958a1cddb86c2abd705b2d4fd36b51037f6879))
 * **[#313](https://github.com/Unleash/unleash-edge/issues/313)**
    - make edge log bad request information from upstream ([`e77ea18`](https://github.com/Unleash/unleash-edge/commit/e77ea1846ae01e9489c048db64c87e53ebdb6fb0))
 * **[#319](https://github.com/Unleash/unleash-edge/issues/319)**
    - bump yggdrasil version ([`3f94d5b`](https://github.com/Unleash/unleash-edge/commit/3f94d5bf593daa34e671e972789a213206eea92e))
 * **Uncategorized**
    - Release unleash-edge v15.0.0 ([`6f56a77`](https://github.com/Unleash/unleash-edge/commit/6f56a77013a4aa9cd820076bb7e8b0787394dab6))
    - Update dependency pointer to 14.0.0 ([`8279ec0`](https://github.com/Unleash/unleash-edge/commit/8279ec0c411586c91b1f7d5214be963a47bff9e0))
    - Release unleash-edge v14.0.0 ([`e0cdb5c`](https://github.com/Unleash/unleash-edge/commit/e0cdb5c7de1c4bf43776f750099072df3a36ae1a))
    - remove dotenv and bump ahash to a non-yanked version ([`0bda1cf`](https://github.com/Unleash/unleash-edge/commit/0bda1cfd8fc157c36a3486acc2d949b2fccc15e4))
    - dependencies bump ([`0c6b33a`](https://github.com/Unleash/unleash-edge/commit/0c6b33a1e011b76fd75618e4ba3cb8a52f7e6c2c))
    - bump renovate bot suggestions ([`1b4277e`](https://github.com/Unleash/unleash-edge/commit/1b4277e211e8ece600b482fe49544163bcbf5eb9))
</details>

## 14.0.0 (2023-10-25)

<csr-id-0bda1cfd8fc157c36a3486acc2d949b2fccc15e4/>
<csr-id-0c6b33a1e011b76fd75618e4ba3cb8a52f7e6c2c/>
<csr-id-1b4277e211e8ece600b482fe49544163bcbf5eb9/>
<csr-id-30958a1cddb86c2abd705b2d4fd36b51037f6879/>

### Chore

 - <csr-id-0bda1cfd8fc157c36a3486acc2d949b2fccc15e4/> remove dotenv and bump ahash to a non-yanked version
 - <csr-id-0c6b33a1e011b76fd75618e4ba3cb8a52f7e6c2c/> dependencies bump
 - <csr-id-1b4277e211e8ece600b482fe49544163bcbf5eb9/> bump renovate bot suggestions
 - <csr-id-30958a1cddb86c2abd705b2d4fd36b51037f6879/> Start work upgrading to opentelemetry 0.20

### New Features

 - <csr-id-e77ea1846ae01e9489c048db64c87e53ebdb6fb0/> make edge log bad request information from upstream

## 13.1.0 (2023-10-16)

### New Features

 - <csr-id-db9fb2d6624efae325416152e3b0ebe2816f2153/> add dependent flag capability by bumping yggdrasil
   * feat: add dependent flag capability by bumping yggdrasil
* Update lockfile

### Bug Fixes

 - <csr-id-4f2adb7f5d6c47dfef2a701d8209c454a8822a3e/> move etag middleware to last in chain to ensure it gets added in gziped responses

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release over the course of 2 calendar days.
 - 3 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#304](https://github.com/Unleash/unleash-edge/issues/304), [#305](https://github.com/Unleash/unleash-edge/issues/305)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#304](https://github.com/Unleash/unleash-edge/issues/304)**
    - move etag middleware to last in chain to ensure it gets added in gziped responses ([`4f2adb7`](https://github.com/Unleash/unleash-edge/commit/4f2adb7f5d6c47dfef2a701d8209c454a8822a3e))
 * **[#305](https://github.com/Unleash/unleash-edge/issues/305)**
    - add dependent flag capability by bumping yggdrasil ([`db9fb2d`](https://github.com/Unleash/unleash-edge/commit/db9fb2d6624efae325416152e3b0ebe2816f2153))
 * **Uncategorized**
    - Release unleash-edge v13.1.0 ([`3c733fc`](https://github.com/Unleash/unleash-edge/commit/3c733fc2beb0095b46e7efc9a08d6c87039f894a))
</details>

## 13.0.2 (2023-10-12)

<csr-id-f5541a14cc2ef69a58513d212b9779eff4e4358d/>

### Chore

 - <csr-id-f5541a14cc2ef69a58513d212b9779eff4e4358d/> bump utoipa and unleash types

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 day passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#302](https://github.com/Unleash/unleash-edge/issues/302)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#302](https://github.com/Unleash/unleash-edge/issues/302)**
    - bump utoipa and unleash types ([`f5541a1`](https://github.com/Unleash/unleash-edge/commit/f5541a14cc2ef69a58513d212b9779eff4e4358d))
 * **Uncategorized**
    - Release unleash-edge v13.0.2 ([`b856507`](https://github.com/Unleash/unleash-edge/commit/b85650717515c2ada85feed389ae8906d368eb00))
</details>

## 13.0.1 (2023-10-10)

### Documentation

 - <csr-id-b8d422a08a0ec00b3ed80ed53e29f694a597afe4/> Add link to feature flags best practices

### Bug Fixes

 - <csr-id-9b6a8906f17438a4332b26a892604359dc3fbaf1/> update Yggdrasil
   Previously, yggdrasil returned a disabled variant if the strategy
   variants representation came back as an empty list instead of null.
   With Yggdrasil 0.6 this is now fixed.
 - <csr-id-90c30e313257a91f640f9d5020cb73004046a97a/> update rust crate reqwest to 0.11.21
 - <csr-id-8d1c294a50c9c939f9365cd9d8e324c0faf512fc/> update rust crate clap to 4.4.6
 - <csr-id-7d3a93b9298304cd4f5ddcf1d51ae6c850fea19e/> update rust crate rustls to 0.21.7

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 6 commits contributed to the release over the course of 7 calendar days.
 - 12 days passed between releases.
 - 5 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 5 unique issues were worked on: [#189](https://github.com/Unleash/unleash-edge/issues/189), [#281](https://github.com/Unleash/unleash-edge/issues/281), [#287](https://github.com/Unleash/unleash-edge/issues/287), [#288](https://github.com/Unleash/unleash-edge/issues/288), [#300](https://github.com/Unleash/unleash-edge/issues/300)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#189](https://github.com/Unleash/unleash-edge/issues/189)**
    - update rust crate clap to 4.4.6 ([`8d1c294`](https://github.com/Unleash/unleash-edge/commit/8d1c294a50c9c939f9365cd9d8e324c0faf512fc))
 * **[#281](https://github.com/Unleash/unleash-edge/issues/281)**
    - update rust crate rustls to 0.21.7 ([`7d3a93b`](https://github.com/Unleash/unleash-edge/commit/7d3a93b9298304cd4f5ddcf1d51ae6c850fea19e))
 * **[#287](https://github.com/Unleash/unleash-edge/issues/287)**
    - Add link to feature flags best practices ([`b8d422a`](https://github.com/Unleash/unleash-edge/commit/b8d422a08a0ec00b3ed80ed53e29f694a597afe4))
 * **[#288](https://github.com/Unleash/unleash-edge/issues/288)**
    - update rust crate reqwest to 0.11.21 ([`90c30e3`](https://github.com/Unleash/unleash-edge/commit/90c30e313257a91f640f9d5020cb73004046a97a))
 * **[#300](https://github.com/Unleash/unleash-edge/issues/300)**
    - update Yggdrasil ([`9b6a890`](https://github.com/Unleash/unleash-edge/commit/9b6a8906f17438a4332b26a892604359dc3fbaf1))
 * **Uncategorized**
    - Release unleash-edge v13.0.1 ([`cae9a71`](https://github.com/Unleash/unleash-edge/commit/cae9a7173401bbee9952c547c535aab5503550fb))
</details>

## 13.0.0 (2023-09-27)

<csr-id-0aa7b4a2214dd0060ba01402f7f4cb074918d6cb/>
<csr-id-629c4b8dba5aedd0f4e0520ad01d2ec5c85d03c4/>

### Chore

 - <csr-id-0aa7b4a2214dd0060ba01402f7f4cb074918d6cb/> Bump tokio,clap,shadow,serde_json to latest
 - <csr-id-629c4b8dba5aedd0f4e0520ad01d2ec5c85d03c4/> update rust crate actix-http to 3.4.0

### Bug Fixes

 - <csr-id-df73932e769efe9ff42f669580d7fb1de1dd31de/> No longer return wrong feature toggle
 - <csr-id-998314337ca42eab01881b2274e6f8012f429bd3/> archived toggles now removed from edge
   Previously, we made a best effort to keep all known data when an update
   came in. Realizing that the Unleash API currently does not allow for
   partial project tokens. Any refreshed data for a single project, can
   drop the existing project and replace it with the incoming
 - <csr-id-00661c4ac5db8cdb6ba95d992ecd1507a9124677/> use the validated token to calculate flags to return
   * fix(#274): use the validated token to calculate flags to return
* fix(#274): update more uses
* Update server/src/client_api.rs

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 6 commits contributed to the release.
 - 9 days passed between releases.
 - 5 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 3 unique issues were worked on: [#254](https://github.com/Unleash/unleash-edge/issues/254), [#275](https://github.com/Unleash/unleash-edge/issues/275), [#283](https://github.com/Unleash/unleash-edge/issues/283)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#254](https://github.com/Unleash/unleash-edge/issues/254)**
    - update rust crate actix-http to 3.4.0 ([`629c4b8`](https://github.com/Unleash/unleash-edge/commit/629c4b8dba5aedd0f4e0520ad01d2ec5c85d03c4))
 * **[#275](https://github.com/Unleash/unleash-edge/issues/275)**
    - use the validated token to calculate flags to return ([`00661c4`](https://github.com/Unleash/unleash-edge/commit/00661c4ac5db8cdb6ba95d992ecd1507a9124677))
 * **[#283](https://github.com/Unleash/unleash-edge/issues/283)**
    - No longer return wrong feature toggle ([`df73932`](https://github.com/Unleash/unleash-edge/commit/df73932e769efe9ff42f669580d7fb1de1dd31de))
 * **Uncategorized**
    - Release unleash-edge v13.0.0 ([`9e32cd9`](https://github.com/Unleash/unleash-edge/commit/9e32cd94583795c057dd8e13969f529f1a60fd74))
    - archived toggles now removed from edge ([`9983143`](https://github.com/Unleash/unleash-edge/commit/998314337ca42eab01881b2274e6f8012f429bd3))
    - Bump tokio,clap,shadow,serde_json to latest ([`0aa7b4a`](https://github.com/Unleash/unleash-edge/commit/0aa7b4a2214dd0060ba01402f7f4cb074918d6cb))
</details>

## 12.0.0 (2023-09-18)

### Documentation

 - <csr-id-16ed8c027bc68941c0b36173d3717668f64fc75f/> updated to reference 12.0.0 version

### New Features

 - <csr-id-62af705c24862315ac279237d7ed23ed9fe9d957/> Added ready subcommand to cli

### Bug Fixes

 - <csr-id-1cc30b700a6b1b6df520f1c100a90a401d7660d4/> Update docs about environment in offline mode
   * fix: Update docs about environment in offline mode

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 4 commits contributed to the release.
 - 4 days passed between releases.
 - 3 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#267](https://github.com/Unleash/unleash-edge/issues/267), [#270](https://github.com/Unleash/unleash-edge/issues/270)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#267](https://github.com/Unleash/unleash-edge/issues/267)**
    - Added ready subcommand to cli ([`62af705`](https://github.com/Unleash/unleash-edge/commit/62af705c24862315ac279237d7ed23ed9fe9d957))
 * **[#270](https://github.com/Unleash/unleash-edge/issues/270)**
    - Update docs about environment in offline mode ([`1cc30b7`](https://github.com/Unleash/unleash-edge/commit/1cc30b700a6b1b6df520f1c100a90a401d7660d4))
 * **Uncategorized**
    - Release unleash-edge v12.0.0 ([`24fd449`](https://github.com/Unleash/unleash-edge/commit/24fd449a8e53fcd742ef34b6c5e0abfbda6162a4))
    - updated to reference 12.0.0 version ([`16ed8c0`](https://github.com/Unleash/unleash-edge/commit/16ed8c027bc68941c0b36173d3717668f64fc75f))
</details>

## 11.0.2 (2023-09-14)

### Bug Fixes

 - <csr-id-ac60f5dd3ac26ecef9befbd79b5a01b07ffb30f3/> allow startup tokens to continue to validate against unleash until they succeed

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#264](https://github.com/Unleash/unleash-edge/issues/264)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#264](https://github.com/Unleash/unleash-edge/issues/264)**
    - allow startup tokens to continue to validate against unleash until they succeed ([`ac60f5d`](https://github.com/Unleash/unleash-edge/commit/ac60f5dd3ac26ecef9befbd79b5a01b07ffb30f3))
 * **Uncategorized**
    - Release unleash-edge v11.0.2 ([`7715e84`](https://github.com/Unleash/unleash-edge/commit/7715e84b44c414358c8bebcdf77f72951ff47e49))
</details>

## 11.0.1 (2023-09-13)

<csr-id-2d124017e7b282b65ed29adb71dac450961066ea/>

### Chore

 - <csr-id-2d124017e7b282b65ed29adb71dac450961066ea/> moved redis to bottom of edge cli struct

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release unleash-edge v11.0.1 ([`cabbf42`](https://github.com/Unleash/unleash-edge/commit/cabbf4207e505e084a0c6709e56bb694f4ece140))
    - moved redis to bottom of edge cli struct ([`2d12401`](https://github.com/Unleash/unleash-edge/commit/2d124017e7b282b65ed29adb71dac450961066ea))
</details>

## 11.0.0 (2023-09-13)

<csr-id-77a078d71cd826f07778ebc54153579a32dbaf53/>

### Chore

 - <csr-id-77a078d71cd826f07778ebc54153579a32dbaf53/> Upgrade to actix 4.4 and rustls 0.21

### New Features

 - <csr-id-6da7d98617394b654fb660912af32a892c4b3546/> expose timeouts in CLI args for connecting to Edge and/or upstream
 - <csr-id-022b361e24f6425028ae7f4b518163477305b30d/> more information in error logs
 - <csr-id-3fff36356e5d8557e590b07399a060ad6033bde8/> add /internal-backstage/ready endpoint
 - <csr-id-29a2584cbbf59c7e9089654859f4579f8138ef79/> added token info endpoint

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 6 commits contributed to the release over the course of 4 calendar days.
 - 5 days passed between releases.
 - 5 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 5 unique issues were worked on: [#245](https://github.com/Unleash/unleash-edge/issues/245), [#250](https://github.com/Unleash/unleash-edge/issues/250), [#252](https://github.com/Unleash/unleash-edge/issues/252), [#253](https://github.com/Unleash/unleash-edge/issues/253), [#262](https://github.com/Unleash/unleash-edge/issues/262)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#245](https://github.com/Unleash/unleash-edge/issues/245)**
    - Upgrade to actix 4.4 and rustls 0.21 ([`77a078d`](https://github.com/Unleash/unleash-edge/commit/77a078d71cd826f07778ebc54153579a32dbaf53))
 * **[#250](https://github.com/Unleash/unleash-edge/issues/250)**
    - added token info endpoint ([`29a2584`](https://github.com/Unleash/unleash-edge/commit/29a2584cbbf59c7e9089654859f4579f8138ef79))
 * **[#252](https://github.com/Unleash/unleash-edge/issues/252)**
    - add /internal-backstage/ready endpoint ([`3fff363`](https://github.com/Unleash/unleash-edge/commit/3fff36356e5d8557e590b07399a060ad6033bde8))
 * **[#253](https://github.com/Unleash/unleash-edge/issues/253)**
    - more information in error logs ([`022b361`](https://github.com/Unleash/unleash-edge/commit/022b361e24f6425028ae7f4b518163477305b30d))
 * **[#262](https://github.com/Unleash/unleash-edge/issues/262)**
    - expose timeouts in CLI args for connecting to Edge and/or upstream ([`6da7d98`](https://github.com/Unleash/unleash-edge/commit/6da7d98617394b654fb660912af32a892c4b3546))
 * **Uncategorized**
    - Release unleash-edge v11.0.0 ([`dfdbf99`](https://github.com/Unleash/unleash-edge/commit/dfdbf99708161f23cbf0f849ba781cc833ab8fcb))
</details>

## 10.0.0 (2023-09-08)

### New Features

 - <csr-id-510073784335e8d8ec8f8e4cc988bc2aad176c8e/> add hot reloading and an optional, simpler file format to offline mode

### Bug Fixes

 - <csr-id-2025d5114d9e47a5b820d065642d3df697223f38/> make fe tokens respect token cache

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 15 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#241](https://github.com/Unleash/unleash-edge/issues/241), [#242](https://github.com/Unleash/unleash-edge/issues/242)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#241](https://github.com/Unleash/unleash-edge/issues/241)**
    - make fe tokens respect token cache ([`2025d51`](https://github.com/Unleash/unleash-edge/commit/2025d5114d9e47a5b820d065642d3df697223f38))
 * **[#242](https://github.com/Unleash/unleash-edge/issues/242)**
    - add hot reloading and an optional, simpler file format to offline mode ([`5100737`](https://github.com/Unleash/unleash-edge/commit/510073784335e8d8ec8f8e4cc988bc2aad176c8e))
 * **Uncategorized**
    - Release unleash-edge v10.0.0 ([`e76da04`](https://github.com/Unleash/unleash-edge/commit/e76da0414d2b37518ce218baa7fae51424fdeaa6))
</details>

## 9.0.0 (2023-08-23)

<csr-id-a6f1829102c671ebbab15f37502bc40f21616da6/>

### Chore

 - <csr-id-a6f1829102c671ebbab15f37502bc40f21616da6/> remove experimental post features endpoint

### New Features

 - <csr-id-af6c3a2079134acca6ae2739bd28aad61cb7f0ae/> add --disable-all-endpoint flag for disabling proxy /all endpoint

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release over the course of 8 calendar days.
 - 26 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#237](https://github.com/Unleash/unleash-edge/issues/237), [#238](https://github.com/Unleash/unleash-edge/issues/238)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#237](https://github.com/Unleash/unleash-edge/issues/237)**
    - add --disable-all-endpoint flag for disabling proxy /all endpoint ([`af6c3a2`](https://github.com/Unleash/unleash-edge/commit/af6c3a2079134acca6ae2739bd28aad61cb7f0ae))
 * **[#238](https://github.com/Unleash/unleash-edge/issues/238)**
    - remove experimental post features endpoint ([`a6f1829`](https://github.com/Unleash/unleash-edge/commit/a6f1829102c671ebbab15f37502bc40f21616da6))
 * **Uncategorized**
    - Release unleash-edge v9.0.0 ([`40a6a38`](https://github.com/Unleash/unleash-edge/commit/40a6a38a51a8422ca2dd593bb56a11f4598e350e))
</details>

## 8.1.0 (2023-07-27)

<csr-id-9d2271827f7c895acea280463011e638e3dd7dd4/>
<csr-id-d0b0b66d8c608ea742137c7647317fe876527ec9/>

### Chore

 - <csr-id-9d2271827f7c895acea280463011e638e3dd7dd4/> bumps yggdrasil and unleash-types to allow the usage of strategy variants
 - <csr-id-d0b0b66d8c608ea742137c7647317fe876527ec9/> updated README to point to newest edge docker container

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release over the course of 14 calendar days.
 - 14 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#231](https://github.com/Unleash/unleash-edge/issues/231)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#231](https://github.com/Unleash/unleash-edge/issues/231)**
    - bumps yggdrasil and unleash-types to allow the usage of strategy variants ([`9d22718`](https://github.com/Unleash/unleash-edge/commit/9d2271827f7c895acea280463011e638e3dd7dd4))
 * **Uncategorized**
    - Release unleash-edge v8.1.0 ([`db512f8`](https://github.com/Unleash/unleash-edge/commit/db512f81b8d2bb355acd921ac3f046b7204e351d))
    - updated README to point to newest edge docker container ([`d0b0b66`](https://github.com/Unleash/unleash-edge/commit/d0b0b66d8c608ea742137c7647317fe876527ec9))
</details>

## 8.0.1 (2023-07-13)

<csr-id-263d56c6746e141610e54cabb3a8861614ee7e0a/>

### Chore

 - <csr-id-263d56c6746e141610e54cabb3a8861614ee7e0a/> Prepare for release of 8.0.1

### New Features

 - <csr-id-5d63562e65225139c1fb67c715181896b3f982f8/> added timing for client feature fetch operations

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 14 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#228](https://github.com/Unleash/unleash-edge/issues/228)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#228](https://github.com/Unleash/unleash-edge/issues/228)**
    - added timing for client feature fetch operations ([`5d63562`](https://github.com/Unleash/unleash-edge/commit/5d63562e65225139c1fb67c715181896b3f982f8))
 * **Uncategorized**
    - Release unleash-edge v8.0.1 ([`74c6801`](https://github.com/Unleash/unleash-edge/commit/74c68016f3cac5d78bf30dd593083327e32ce3d1))
    - Prepare for release of 8.0.1 ([`263d56c`](https://github.com/Unleash/unleash-edge/commit/263d56c6746e141610e54cabb3a8861614ee7e0a))
</details>

## 8.0.0 (2023-06-28)

<csr-id-a85c2f3911b5cffb6ccee78a74ffa4ece61cebc8/>
<csr-id-9dd9930d22cf259a20b8168d203c3919df019921/>

### Chore

 - <csr-id-a85c2f3911b5cffb6ccee78a74ffa4ece61cebc8/> reduce public api for a number of functions/structs that should never have been public anyway
 - <csr-id-9dd9930d22cf259a20b8168d203c3919df019921/> bump unleash-types to no longer serialize nulls

### Bug Fixes

 - <csr-id-61306041de7f58584ee2ab8d310b12a00f8eeb87/> log levels were too high

### Performance

 - <csr-id-4c03cba94693fe5ede4b7d16fb8ee00ec2d4e572/> improve memory usage during request lifecycle

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 6 commits contributed to the release over the course of 1 calendar day.
 - 5 days passed between releases.
 - 4 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 3 unique issues were worked on: [#218](https://github.com/Unleash/unleash-edge/issues/218), [#220](https://github.com/Unleash/unleash-edge/issues/220), [#221](https://github.com/Unleash/unleash-edge/issues/221)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#218](https://github.com/Unleash/unleash-edge/issues/218)**
    - perf/remove unnecessary clone ([`5808004`](https://github.com/Unleash/unleash-edge/commit/5808004aa4725bb8debd5d9150177c910b63d733))
 * **[#220](https://github.com/Unleash/unleash-edge/issues/220)**
    - improve memory usage during request lifecycle ([`4c03cba`](https://github.com/Unleash/unleash-edge/commit/4c03cba94693fe5ede4b7d16fb8ee00ec2d4e572))
 * **[#221](https://github.com/Unleash/unleash-edge/issues/221)**
    - reduce public api for a number of functions/structs that should never have been public anyway ([`a85c2f3`](https://github.com/Unleash/unleash-edge/commit/a85c2f3911b5cffb6ccee78a74ffa4ece61cebc8))
 * **Uncategorized**
    - Release unleash-edge v8.0.0 ([`16a34df`](https://github.com/Unleash/unleash-edge/commit/16a34dfd27b4e85abf44d440333b5fb0477d2aa3))
    - bump unleash-types to no longer serialize nulls ([`9dd9930`](https://github.com/Unleash/unleash-edge/commit/9dd9930d22cf259a20b8168d203c3919df019921))
    - log levels were too high ([`6130604`](https://github.com/Unleash/unleash-edge/commit/61306041de7f58584ee2ab8d310b12a00f8eeb87))
</details>

## 7.0.1 (2023-06-23)

<csr-id-0920dad3c42cf8284cf21899a8a5f392271acca9/>

### Chore

 - <csr-id-0920dad3c42cf8284cf21899a8a5f392271acca9/> allows resolving a single toggle to do that instead of iterating the whole hashmap

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 10 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#215](https://github.com/Unleash/unleash-edge/issues/215)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#215](https://github.com/Unleash/unleash-edge/issues/215)**
    - allows resolving a single toggle to do that instead of iterating the whole hashmap ([`0920dad`](https://github.com/Unleash/unleash-edge/commit/0920dad3c42cf8284cf21899a8a5f392271acca9))
 * **Uncategorized**
    - Release unleash-edge v7.0.1 ([`92c882e`](https://github.com/Unleash/unleash-edge/commit/92c882e71d2a7747b9b10913757d8767b83241f0))
</details>

## 7.0.0 (2023-06-12)

<csr-id-21178ab0934176e7c1aac9a9093253b806acd399/>

### Chore

 - <csr-id-21178ab0934176e7c1aac9a9093253b806acd399/> remove unneeded import

### New Features

<csr-id-38b36e8af9e1560bc7ece1f644a0349257bf1a36/>
<csr-id-baecc67a5f81fa1c76798c320c65a5e6dbd5b061/>

 - <csr-id-2fcfcc54cc61ae277c8b1b66fe9d8e619ab47494/> trust-proxy - resolving peer ip for context
   * Added trust proxy for enriching context

### Bug Fixes

 - <csr-id-6c3942ef330bc6bf04344193d0ab3be9a1a1e3ed/> remove SA token from app_data, it's already added to the FeatureRefresher
 - <csr-id-d6da27a77551ee22d4e406deb6a3351ad013cf1e/> Don't log the SA token on startup

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 7 commits contributed to the release over the course of 4 calendar days.
 - 5 days passed between releases.
 - 6 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 3 unique issues were worked on: [#206](https://github.com/Unleash/unleash-edge/issues/206), [#208](https://github.com/Unleash/unleash-edge/issues/208), [#210](https://github.com/Unleash/unleash-edge/issues/210)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#206](https://github.com/Unleash/unleash-edge/issues/206)**
    - Use service account to create client tokens ([`baecc67`](https://github.com/Unleash/unleash-edge/commit/baecc67a5f81fa1c76798c320c65a5e6dbd5b061))
 * **[#208](https://github.com/Unleash/unleash-edge/issues/208)**
    - You can now use tcp or tls as schemes for Redis ([`38b36e8`](https://github.com/Unleash/unleash-edge/commit/38b36e8af9e1560bc7ece1f644a0349257bf1a36))
 * **[#210](https://github.com/Unleash/unleash-edge/issues/210)**
    - remove unneeded import ([`21178ab`](https://github.com/Unleash/unleash-edge/commit/21178ab0934176e7c1aac9a9093253b806acd399))
 * **Uncategorized**
    - Release unleash-edge v7.0.0 ([`e6b53a0`](https://github.com/Unleash/unleash-edge/commit/e6b53a0a0e61b98924315f86bc1f4d4d3ea9c317))
    - trust-proxy - resolving peer ip for context ([`2fcfcc5`](https://github.com/Unleash/unleash-edge/commit/2fcfcc54cc61ae277c8b1b66fe9d8e619ab47494))
    - remove SA token from app_data, it's already added to the FeatureRefresher ([`6c3942e`](https://github.com/Unleash/unleash-edge/commit/6c3942ef330bc6bf04344193d0ab3be9a1a1e3ed))
    - Don't log the SA token on startup ([`d6da27a`](https://github.com/Unleash/unleash-edge/commit/d6da27a77551ee22d4e406deb6a3351ad013cf1e))
</details>

## 6.0.0 (2023-06-07)

<csr-id-79613201c810435b8d01696af3864f065c5f0a9b/>
<csr-id-60296f3f8ddfdd5b187345f776d65fae58870cf2/>

### Chore

 - <csr-id-79613201c810435b8d01696af3864f065c5f0a9b/> update README for new release
 - <csr-id-60296f3f8ddfdd5b187345f776d65fae58870cf2/> bump versions

### Documentation

 - <csr-id-786215241cca06b0bbb759633bd40c00401cc19e/> Document that tokens cli arg allows multiple comma-separated values

### New Features

 - <csr-id-107467468f6f5875fa8ed4db456909a6bb17b89d/> add multiple env variables for configuring Redis
   * docs: Added auto generation of markdown help

### Bug Fixes

 - <csr-id-6fe165a6249290e043d21232059e4153436b4fde/> update rust crate chrono to 0.4.26
 - <csr-id-46fd89353a87e6621d8d938dee7023e001ed52b0/> update rust crate unleash-yggdrasil to 0.5.7

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 8 commits contributed to the release over the course of 2 calendar days.
 - 5 days passed between releases.
 - 6 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 6 unique issues were worked on: [#194](https://github.com/Unleash/unleash-edge/issues/194), [#201](https://github.com/Unleash/unleash-edge/issues/201), [#202](https://github.com/Unleash/unleash-edge/issues/202), [#203](https://github.com/Unleash/unleash-edge/issues/203), [#204](https://github.com/Unleash/unleash-edge/issues/204), [#205](https://github.com/Unleash/unleash-edge/issues/205)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#194](https://github.com/Unleash/unleash-edge/issues/194)**
    - update rust crate chrono to 0.4.26 ([`6fe165a`](https://github.com/Unleash/unleash-edge/commit/6fe165a6249290e043d21232059e4153436b4fde))
 * **[#201](https://github.com/Unleash/unleash-edge/issues/201)**
    - update rust crate unleash-yggdrasil to 0.5.7 ([`46fd893`](https://github.com/Unleash/unleash-edge/commit/46fd89353a87e6621d8d938dee7023e001ed52b0))
 * **[#202](https://github.com/Unleash/unleash-edge/issues/202)**
    - add multiple env variables for configuring Redis ([`1074674`](https://github.com/Unleash/unleash-edge/commit/107467468f6f5875fa8ed4db456909a6bb17b89d))
 * **[#203](https://github.com/Unleash/unleash-edge/issues/203)**
    - bump versions ([`60296f3`](https://github.com/Unleash/unleash-edge/commit/60296f3f8ddfdd5b187345f776d65fae58870cf2))
 * **[#204](https://github.com/Unleash/unleash-edge/issues/204)**
    - Task/healthcheck subcommand ([`5253f5e`](https://github.com/Unleash/unleash-edge/commit/5253f5e62704432b1cdaf46a95c9af78b7d5cc96))
 * **[#205](https://github.com/Unleash/unleash-edge/issues/205)**
    - Document that tokens cli arg allows multiple comma-separated values ([`7862152`](https://github.com/Unleash/unleash-edge/commit/786215241cca06b0bbb759633bd40c00401cc19e))
 * **Uncategorized**
    - Release unleash-edge v6.0.0 ([`3ab7074`](https://github.com/Unleash/unleash-edge/commit/3ab70749ecbe19aa0a61b50090eca3af80f64e91))
    - update README for new release ([`7961320`](https://github.com/Unleash/unleash-edge/commit/79613201c810435b8d01696af3864f065c5f0a9b))
</details>

## 5.0.0 (2023-06-01)

### New Features

 - <csr-id-4c187c622a78b7a91b8b3d868e51c51ddd0777c1/> makes a post to api/client/features possible by setting a cli arg

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 2 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#197](https://github.com/Unleash/unleash-edge/issues/197)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#197](https://github.com/Unleash/unleash-edge/issues/197)**
    - makes a post to api/client/features possible by setting a cli arg ([`4c187c6`](https://github.com/Unleash/unleash-edge/commit/4c187c622a78b7a91b8b3d868e51c51ddd0777c1))
 * **Uncategorized**
    - Release unleash-edge v5.0.0 ([`cad3589`](https://github.com/Unleash/unleash-edge/commit/cad3589b28c74f1cb753acea2243aedeb1445268))
</details>

## 4.1.1 (2023-05-30)

### Bug Fixes

 - <csr-id-d60702d1693d4723a22443245ffe02e0771cae82/> pulls in fixes from Unleash Types so that Segments correctly update

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 6 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#196](https://github.com/Unleash/unleash-edge/issues/196)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#196](https://github.com/Unleash/unleash-edge/issues/196)**
    - pulls in fixes from Unleash Types so that Segments correctly update ([`d60702d`](https://github.com/Unleash/unleash-edge/commit/d60702d1693d4723a22443245ffe02e0771cae82))
 * **Uncategorized**
    - Release unleash-edge v4.1.1 ([`2464099`](https://github.com/Unleash/unleash-edge/commit/2464099f18c8e5d9c82b377137a1e7679556fdae))
</details>

## 4.1.0 (2023-05-23)

### New Features

 - <csr-id-108005e4160ee70463d3b7434855426a874407dc/> add base uri path to server
   * feat: add base uri path to server
* docs: update docs to include new arg
* test: add base_path integration test
* test: add another endpoint that assumes 403
* test: use offline mode instead

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 5 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#191](https://github.com/Unleash/unleash-edge/issues/191)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#191](https://github.com/Unleash/unleash-edge/issues/191)**
    - add base uri path to server ([`108005e`](https://github.com/Unleash/unleash-edge/commit/108005e4160ee70463d3b7434855426a874407dc))
 * **Uncategorized**
    - Release unleash-edge v4.1.0 ([`36e538b`](https://github.com/Unleash/unleash-edge/commit/36e538b3594da91047a0626b5811a869c75328b8))
</details>

## 4.0.3 (2023-05-18)

### Bug Fixes

 - <csr-id-808db80fcc68606f540a0339acb8cd757934ecdb/> allow multiple client tokens at startup

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#188](https://github.com/Unleash/unleash-edge/issues/188)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#188](https://github.com/Unleash/unleash-edge/issues/188)**
    - allow multiple client tokens at startup ([`808db80`](https://github.com/Unleash/unleash-edge/commit/808db80fcc68606f540a0339acb8cd757934ecdb))
 * **Uncategorized**
    - Release unleash-edge v4.0.3 ([`fee159c`](https://github.com/Unleash/unleash-edge/commit/fee159c0bdf5dc867b704b7a2bdc2c46fbdcf1d7))
</details>

## 4.0.2 (2023-05-17)

<csr-id-447baab59e4488565e1f4b28613e9a60c2ef4af7/>

### Chore

 - <csr-id-447baab59e4488565e1f4b28613e9a60c2ef4af7/> bump yggdrasil to pull through fix for rollout not working with random

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 4 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#187](https://github.com/Unleash/unleash-edge/issues/187)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#187](https://github.com/Unleash/unleash-edge/issues/187)**
    - bump yggdrasil to pull through fix for rollout not working with random ([`447baab`](https://github.com/Unleash/unleash-edge/commit/447baab59e4488565e1f4b28613e9a60c2ef4af7))
 * **Uncategorized**
    - Release unleash-edge v4.0.2 ([`940bb5b`](https://github.com/Unleash/unleash-edge/commit/940bb5b7376aed092922df87c40ff8198504d4a6))
</details>

## 4.0.1 (2023-05-12)

<csr-id-4984c3eb039837f0bdfa85f94e8129a03b2675a4/>
<csr-id-5b821f831db0dd6d4e4c5affd36624c0929268af/>

### Chore

 - <csr-id-4984c3eb039837f0bdfa85f94e8129a03b2675a4/> allow output from bad requests to respond with the error rather than just the status code
 - <csr-id-5b821f831db0dd6d4e4c5affd36624c0929268af/> output logs for any response from feature query in debug output

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 1 day passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#184](https://github.com/Unleash/unleash-edge/issues/184), [#185](https://github.com/Unleash/unleash-edge/issues/185)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#184](https://github.com/Unleash/unleash-edge/issues/184)**
    - allow output from bad requests to respond with the error rather than just the status code ([`4984c3e`](https://github.com/Unleash/unleash-edge/commit/4984c3eb039837f0bdfa85f94e8129a03b2675a4))
 * **[#185](https://github.com/Unleash/unleash-edge/issues/185)**
    - output logs for any response from feature query in debug output ([`5b821f8`](https://github.com/Unleash/unleash-edge/commit/5b821f831db0dd6d4e4c5affd36624c0929268af))
 * **Uncategorized**
    - Release unleash-edge v4.0.1 ([`c28ca3f`](https://github.com/Unleash/unleash-edge/commit/c28ca3f2a26557da6431c1a9f56941ec99388342))
</details>

## 4.0.0 (2023-05-11)

<csr-id-b5930bcc55d9e241b1fe29002d5cfb8f9191407f/>

### New Features

 - <csr-id-72656280a07c2d2c7729f9f25e9894f22f276ae5/> Add more visible info and warn logging for http errors

### Bug Fixes

 - <csr-id-bb09da9d9f2545a9ab8efda93a4ec270739f07ae/> update rust crate clap to 4.2.7

### Other

 - <csr-id-b5930bcc55d9e241b1fe29002d5cfb8f9191407f/> Prepare for 4.0.0 release

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 4 commits contributed to the release.
 - 6 days passed between releases.
 - 3 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#171](https://github.com/Unleash/unleash-edge/issues/171), [#182](https://github.com/Unleash/unleash-edge/issues/182)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#171](https://github.com/Unleash/unleash-edge/issues/171)**
    - update rust crate clap to 4.2.7 ([`bb09da9`](https://github.com/Unleash/unleash-edge/commit/bb09da9d9f2545a9ab8efda93a4ec270739f07ae))
 * **[#182](https://github.com/Unleash/unleash-edge/issues/182)**
    - Add more visible info and warn logging for http errors ([`7265628`](https://github.com/Unleash/unleash-edge/commit/72656280a07c2d2c7729f9f25e9894f22f276ae5))
 * **Uncategorized**
    - Release unleash-edge v4.0.0 ([`7d43885`](https://github.com/Unleash/unleash-edge/commit/7d438852ba3ea4f3a1fbef2359a5398a8aa6da22))
    - Prepare for 4.0.0 release ([`b5930bc`](https://github.com/Unleash/unleash-edge/commit/b5930bcc55d9e241b1fe29002d5cfb8f9191407f))
</details>

## 3.0.0 (2023-05-05)

### Documentation

 - <csr-id-effe175168bc59fddd04284a4217edcaddb0a714/> Prepare README for 3.0

### New Features

 - <csr-id-9f01316d6ac029d8b5b25140f54827a9627026e7/> Client TLS Identification

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 2 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#176](https://github.com/Unleash/unleash-edge/issues/176)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#176](https://github.com/Unleash/unleash-edge/issues/176)**
    - Client TLS Identification ([`9f01316`](https://github.com/Unleash/unleash-edge/commit/9f01316d6ac029d8b5b25140f54827a9627026e7))
 * **Uncategorized**
    - Release unleash-edge v3.0.0 ([`a6779cd`](https://github.com/Unleash/unleash-edge/commit/a6779cdb17f676ff278c3182e66284c3800955df))
    - Prepare README for 3.0 ([`effe175`](https://github.com/Unleash/unleash-edge/commit/effe175168bc59fddd04284a4217edcaddb0a714))
</details>

## 2.0.2 (2023-05-02)

<csr-id-dde64c8993e0c7003c544a2a68a52b1867b55ed2/>

### Bug Fixes

 - <csr-id-dfb191093063d676323d840614cd3e381cb4aa8a/> Handle both upper and lowercase of apitokentype.
   Unleash has suddenly started returning token type with uppercase.
   This PR makes us handle both UPPER and lower case for token type.
   
   The real fix would be for Unleash to obey its own contract with
   lowercase for token types, but this fix makes us more tolerant to
   mistakes in Unleash code.

### Other

 - <csr-id-dde64c8993e0c7003c544a2a68a52b1867b55ed2/> prepare for release

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 11 days passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#174](https://github.com/Unleash/unleash-edge/issues/174)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#174](https://github.com/Unleash/unleash-edge/issues/174)**
    - Handle both upper and lowercase of apitokentype. ([`dfb1910`](https://github.com/Unleash/unleash-edge/commit/dfb191093063d676323d840614cd3e381cb4aa8a))
 * **Uncategorized**
    - Release unleash-edge v2.0.2 ([`357b407`](https://github.com/Unleash/unleash-edge/commit/357b4070d53124f8fc06627c30ae4e43dd9f9594))
    - prepare for release ([`dde64c8`](https://github.com/Unleash/unleash-edge/commit/dde64c8993e0c7003c544a2a68a52b1867b55ed2))
</details>

## 2.0.1 (2023-04-20)

### Bug Fixes

 - <csr-id-208ba30133348f8cb3ae4303415ac9c1484f03c5/> Use split_once.
   We had forgotten that `:` is a valid part of a header value, so
   splitting on `'` ended up splitting into too many parts and thus failing
   the parser. This PR changes to use split_once, which splits on first
   occurrence. Since Header Names are not allowed to contains `:` this will
   be fine
 - <csr-id-007a061b6c0eaa52de3eee81e8cadc0530562751/> update rust crate clap to 4.2.4

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 4 commits contributed to the release over the course of 1 calendar day.
 - 1 day passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#140](https://github.com/Unleash/unleash-edge/issues/140), [#164](https://github.com/Unleash/unleash-edge/issues/164)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#140](https://github.com/Unleash/unleash-edge/issues/140)**
    - update rust crate clap to 4.2.4 ([`007a061`](https://github.com/Unleash/unleash-edge/commit/007a061b6c0eaa52de3eee81e8cadc0530562751))
 * **[#164](https://github.com/Unleash/unleash-edge/issues/164)**
    - Use split_once. ([`208ba30`](https://github.com/Unleash/unleash-edge/commit/208ba30133348f8cb3ae4303415ac9c1484f03c5))
 * **Uncategorized**
    - Release unleash-edge v2.0.1 ([`6c7316b`](https://github.com/Unleash/unleash-edge/commit/6c7316b79f335b3fbbfb156a2ed00bbf95b0018f))
    - Update dependency status link ([`fc19857`](https://github.com/Unleash/unleash-edge/commit/fc1985758eb32885a2c970e7707623eea7bcabcd))
</details>

## 2.0.0 (2023-04-19)

### Bug Fixes

 - <csr-id-33d412a7fc304fb55203c8132ee59566e05b9874/> Building context parameters using map syntax

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 day passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#160](https://github.com/Unleash/unleash-edge/issues/160)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#160](https://github.com/Unleash/unleash-edge/issues/160)**
    - Building context parameters using map syntax ([`33d412a`](https://github.com/Unleash/unleash-edge/commit/33d412a7fc304fb55203c8132ee59566e05b9874))
 * **Uncategorized**
    - Release unleash-edge v2.0.0 ([`7438b56`](https://github.com/Unleash/unleash-edge/commit/7438b567dca06e359bd9eb35dbb7ae9c3d6e5c1b))
</details>

## 1.4.0 (2023-04-17)

### New Features

 - <csr-id-b74027319c87e078b042a28051e52e37dab956a9/> allow cli option to disable ssl verification

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 2 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#157](https://github.com/Unleash/unleash-edge/issues/157)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#157](https://github.com/Unleash/unleash-edge/issues/157)**
    - allow cli option to disable ssl verification ([`b740273`](https://github.com/Unleash/unleash-edge/commit/b74027319c87e078b042a28051e52e37dab956a9))
 * **Uncategorized**
    - Release unleash-edge v1.4.0 ([`0847f7d`](https://github.com/Unleash/unleash-edge/commit/0847f7da0f4a761858b706300ee6048982270a7a))
</details>

## 1.3.1 (2023-04-14)

### Bug Fixes

 - <csr-id-a4930e6897b30e2b00e118a078d45e953190cfc6/> Fixes incorrect parsing of extra arguments.
   Currently we do not parse extra parameters into the properties holder
   for the context. This PR updates to make sure that overflows (properties
   we haven't explicitly defined) get parsed into the properties map

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 day passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#156](https://github.com/Unleash/unleash-edge/issues/156)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#156](https://github.com/Unleash/unleash-edge/issues/156)**
    - Fixes incorrect parsing of extra arguments. ([`a4930e6`](https://github.com/Unleash/unleash-edge/commit/a4930e6897b30e2b00e118a078d45e953190cfc6))
 * **Uncategorized**
    - Release unleash-edge v1.3.1 ([`845aa17`](https://github.com/Unleash/unleash-edge/commit/845aa177220a6ce61091c8861d1342e69f680ace))
</details>

## 1.3.0 (2023-04-13)

<csr-id-9a651efc0393cebeb67e639aa612434606b4c9ed/>
<csr-id-8bba7f47b2204d63409b0220ada78edb6bc156de/>

### Chore

 - <csr-id-9a651efc0393cebeb67e639aa612434606b4c9ed/> bump dependency status link

### Chore

 - <csr-id-8bba7f47b2204d63409b0220ada78edb6bc156de/> added changelog for 1.3.0 release

### Documentation

 - <csr-id-625b0760c66574f94a098885ff94735330a2bb2d/> updated README in server subfolder

### New Features

 - <csr-id-c417cab5698ac1f45e8f640012b06b655abb900d/> Added single feature evaluation endpoint
   For now, we resolve all toggles, waiting for feature improvement in
   Yggdrasil that allows us to query single ResolvedToggle by name
 - <csr-id-bde2d013b9b0e664ce087b0f6d6b979e11454414/> Added support for --custom-client-headers (CUSTOM_CLIENT_HEADERS)

### Bug Fixes

 - <csr-id-d1052f7f913713c488d3cd038709a872cb493c71/> update rust crate serde_json to 1.0.96

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 7 commits contributed to the release over the course of 1 calendar day.
 - 1 day passed between releases.
 - 5 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#151](https://github.com/Unleash/unleash-edge/issues/151), [#154](https://github.com/Unleash/unleash-edge/issues/154)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#151](https://github.com/Unleash/unleash-edge/issues/151)**
    - Added single feature evaluation endpoint ([`c417cab`](https://github.com/Unleash/unleash-edge/commit/c417cab5698ac1f45e8f640012b06b655abb900d))
 * **[#154](https://github.com/Unleash/unleash-edge/issues/154)**
    - update rust crate serde_json to 1.0.96 ([`d1052f7`](https://github.com/Unleash/unleash-edge/commit/d1052f7f913713c488d3cd038709a872cb493c71))
 * **Uncategorized**
    - Release unleash-edge v1.3.0 ([`83a7b97`](https://github.com/Unleash/unleash-edge/commit/83a7b97fe9ddff1871fe8563a96025a63fc91f4d))
    - added changelog for 1.3.0 release ([`8bba7f4`](https://github.com/Unleash/unleash-edge/commit/8bba7f47b2204d63409b0220ada78edb6bc156de))
    - updated README in server subfolder ([`625b076`](https://github.com/Unleash/unleash-edge/commit/625b0760c66574f94a098885ff94735330a2bb2d))
    - bump dependency status link ([`9a651ef`](https://github.com/Unleash/unleash-edge/commit/9a651efc0393cebeb67e639aa612434606b4c9ed))
    - * feat: Add custom headers for clients ([`bde2d01`](https://github.com/Unleash/unleash-edge/commit/bde2d013b9b0e664ce087b0f6d6b979e11454414))
</details>

## v1.2.0 (2023-04-11)

### Documentation

 - <csr-id-26805bb55b25edc4cc2e41f525c7eee71df4cd54/> update dependency links

### New Features

 - <csr-id-ebd63005c3ff2f73da7cb35872bd132d1c953dd7/> add namePrefix filtering support
 - <csr-id-28fff02aaf3bda6305186a323b4a507356bfd6db/> add metrics endpoints for frontend

### Bug Fixes

 - <csr-id-48994233364c9fa7af8dc331dede2da38913922e/> update rust-futures monorepo to 0.3.28

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 5 commits contributed to the release over the course of 11 calendar days.
 - 13 days passed between releases.
 - 4 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 3 unique issues were worked on: [#141](https://github.com/Unleash/unleash-edge/issues/141), [#147](https://github.com/Unleash/unleash-edge/issues/147), [#149](https://github.com/Unleash/unleash-edge/issues/149)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#141](https://github.com/Unleash/unleash-edge/issues/141)**
    - update rust-futures monorepo to 0.3.28 ([`4899423`](https://github.com/Unleash/unleash-edge/commit/48994233364c9fa7af8dc331dede2da38913922e))
 * **[#147](https://github.com/Unleash/unleash-edge/issues/147)**
    - add metrics endpoints for frontend ([`28fff02`](https://github.com/Unleash/unleash-edge/commit/28fff02aaf3bda6305186a323b4a507356bfd6db))
 * **[#149](https://github.com/Unleash/unleash-edge/issues/149)**
    - add namePrefix filtering support ([`ebd6300`](https://github.com/Unleash/unleash-edge/commit/ebd63005c3ff2f73da7cb35872bd132d1c953dd7))
 * **Uncategorized**
    - Release unleash-edge v1.2.0 ([`ab51228`](https://github.com/Unleash/unleash-edge/commit/ab5122837f0476d055eaf007a55c13a715b1fdb3))
    - update dependency links ([`26805bb`](https://github.com/Unleash/unleash-edge/commit/26805bb55b25edc4cc2e41f525c7eee71df4cd54))
</details>

## v1.1.0 (2023-03-29)

### New Features

 - <csr-id-5a7040c3c5787451e31dd3e804946c321ad6805a/> Add client feature endpoint
   Our client api in Unleash server also supports querying single features
   by name in path. This PR adds the necessary endpoint to support this.

### Bug Fixes

 - <csr-id-8e4df8d5b6d8800ad644cac0c6cda7c19386426f/> update rust crate clap to 4.2.0

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#132](https://github.com/Unleash/unleash-edge/issues/132), [#138](https://github.com/Unleash/unleash-edge/issues/138)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#132](https://github.com/Unleash/unleash-edge/issues/132)**
    - update rust crate clap to 4.2.0 ([`8e4df8d`](https://github.com/Unleash/unleash-edge/commit/8e4df8d5b6d8800ad644cac0c6cda7c19386426f))
 * **[#138](https://github.com/Unleash/unleash-edge/issues/138)**
    - Add client feature endpoint ([`5a7040c`](https://github.com/Unleash/unleash-edge/commit/5a7040c3c5787451e31dd3e804946c321ad6805a))
 * **Uncategorized**
    - Release unleash-edge v1.1.0 ([`bcde480`](https://github.com/Unleash/unleash-edge/commit/bcde48011570099463a493f9dbd66162da9b9992))
</details>

## v1.0.2 (2023-03-28)

<csr-id-1ab5962ebc10c8a5f14492fcd28b46e541d2992d/>
<csr-id-b97681b8e9d40afd35b629f0d9b4757c66a637a8/>

### Chore

 - <csr-id-1ab5962ebc10c8a5f14492fcd28b46e541d2992d/> use fewer clones to reduce allocation

### Bug Fixes

 - <csr-id-a858391e9cc7d9bd805a892519f38da6b4be0ebb/> added custom metrics handler to drop dependency
 - <csr-id-f835db09798cdd45181000b194348d7cd1f3ba08/> update rust crate clap to 4.1.13
 - <csr-id-5034f87f9d0d0d38bd8674fd00acc52bf863559a/> update rust crate reqwest to 0.11.15

### Other

 - <csr-id-b97681b8e9d40afd35b629f0d9b4757c66a637a8/> Post appropriately sized metric batches
   * task: Post appropriately sized metric batches
   
   Previously we would save unacknowledged metrics until upstream accepted
   the post. This PR, splits into 90kB chunks, listens for http status
   codes to decide what to do on failure.
   * 400 will cause us to drop the metrics we tried to post
   * 413 would be a surprise, since we already split into chunks to avoid
     just this
   * other status codes will be reinserted to the cache and tried again
     next minute.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 6 commits contributed to the release over the course of 3 calendar days.
 - 5 days passed between releases.
 - 5 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 5 unique issues were worked on: [#117](https://github.com/Unleash/unleash-edge/issues/117), [#121](https://github.com/Unleash/unleash-edge/issues/121), [#122](https://github.com/Unleash/unleash-edge/issues/122), [#127](https://github.com/Unleash/unleash-edge/issues/127), [#135](https://github.com/Unleash/unleash-edge/issues/135)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#117](https://github.com/Unleash/unleash-edge/issues/117)**
    - update rust crate reqwest to 0.11.15 ([`5034f87`](https://github.com/Unleash/unleash-edge/commit/5034f87f9d0d0d38bd8674fd00acc52bf863559a))
 * **[#121](https://github.com/Unleash/unleash-edge/issues/121)**
    - update rust crate clap to 4.1.13 ([`f835db0`](https://github.com/Unleash/unleash-edge/commit/f835db09798cdd45181000b194348d7cd1f3ba08))
 * **[#122](https://github.com/Unleash/unleash-edge/issues/122)**
    - Post appropriately sized metric batches ([`b97681b`](https://github.com/Unleash/unleash-edge/commit/b97681b8e9d40afd35b629f0d9b4757c66a637a8))
 * **[#127](https://github.com/Unleash/unleash-edge/issues/127)**
    - use fewer clones to reduce allocation ([`1ab5962`](https://github.com/Unleash/unleash-edge/commit/1ab5962ebc10c8a5f14492fcd28b46e541d2992d))
 * **[#135](https://github.com/Unleash/unleash-edge/issues/135)**
    - added custom metrics handler to drop dependency ([`a858391`](https://github.com/Unleash/unleash-edge/commit/a858391e9cc7d9bd805a892519f38da6b4be0ebb))
 * **Uncategorized**
    - Release unleash-edge v1.0.2 ([`7014153`](https://github.com/Unleash/unleash-edge/commit/7014153c028e95cf977d206b3741bafea4758bbb))
</details>

## v1.0.1 (2023-03-23)

### Bug Fixes

 - <csr-id-d067f92d42a7a2051ea45683763297bfc20cc7c1/> Save checked tokens even if invalid
   To make sure we don't hammer upstream with validation requests even if
   edge is under heavy load, this PR allows Edge to save validation results
   for invalid tokens as well.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 2 days passed between releases.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#120](https://github.com/Unleash/unleash-edge/issues/120)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#120](https://github.com/Unleash/unleash-edge/issues/120)**
    - Save checked tokens even if invalid ([`d067f92`](https://github.com/Unleash/unleash-edge/commit/d067f92d42a7a2051ea45683763297bfc20cc7c1))
 * **Uncategorized**
    - Release unleash-edge v1.0.1 ([`a470163`](https://github.com/Unleash/unleash-edge/commit/a470163679862369666c90bdc33e4cd4c1bceb55))
</details>

## v1.0.0 (2023-03-20)

<csr-id-584e61bb98e32083996720f9d703341ca0025ed6/>

### New Features

 - <csr-id-1e73fdcbce1786aea9f4a1b1a5a9a188c656e85c/> Client features are hydrated synchronously.
   Previously Edge returned a 503 the first time it saw a new client token.
   It now blocks until it's fetched the data for the new token and then
   returns it.

### Bug Fixes

 - <csr-id-d3dfefc08b4a2bdc837d153e89a17a5025908764/> clone value of cache entry
   When in offline mode, was using DashMap incorrectly. The get function
   returns a ref to the entry, so to get at the actual data you have to
   call .value(). This commit fixes that for the client features api
 - <csr-id-0a9353a95e83b30b46b04047f06f359933306ec7/> update rust crate serde to 1.0.158
 - <csr-id-b5604a34ee23aa17847fb8280c10cababce5ad26/> update rust crate clap to 4.1.11
 - <csr-id-4d704b68b78eb066a03d0c5006979db3189f5d43/> update rust crate clap to 4.1.9

### Other

 - <csr-id-584e61bb98e32083996720f9d703341ca0025ed6/> Return 511 if edge has not hydrated.
   Our client/frontend token separation leads to us having to hydrate
   client features using a client token. If a frontend token comes in that has
   access to a project/environment combination that Edge has not yet seen a
   client token for, this PR now makes Edge consistently return a 511 with
   a body explaining which project and environment the user has to add a
   client token for

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 7 commits contributed to the release over the course of 3 calendar days.
 - 4 days passed between releases.
 - 6 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 5 unique issues were worked on: [#110](https://github.com/Unleash/unleash-edge/issues/110), [#111](https://github.com/Unleash/unleash-edge/issues/111), [#112](https://github.com/Unleash/unleash-edge/issues/112), [#113](https://github.com/Unleash/unleash-edge/issues/113), [#116](https://github.com/Unleash/unleash-edge/issues/116)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#110](https://github.com/Unleash/unleash-edge/issues/110)**
    - update rust crate clap to 4.1.9 ([`4d704b6`](https://github.com/Unleash/unleash-edge/commit/4d704b68b78eb066a03d0c5006979db3189f5d43))
 * **[#111](https://github.com/Unleash/unleash-edge/issues/111)**
    - Return 511 if edge has not hydrated. ([`584e61b`](https://github.com/Unleash/unleash-edge/commit/584e61bb98e32083996720f9d703341ca0025ed6))
 * **[#112](https://github.com/Unleash/unleash-edge/issues/112)**
    - Client features are hydrated synchronously. ([`1e73fdc`](https://github.com/Unleash/unleash-edge/commit/1e73fdcbce1786aea9f4a1b1a5a9a188c656e85c))
 * **[#113](https://github.com/Unleash/unleash-edge/issues/113)**
    - update rust crate clap to 4.1.11 ([`b5604a3`](https://github.com/Unleash/unleash-edge/commit/b5604a34ee23aa17847fb8280c10cababce5ad26))
 * **[#116](https://github.com/Unleash/unleash-edge/issues/116)**
    - update rust crate serde to 1.0.158 ([`0a9353a`](https://github.com/Unleash/unleash-edge/commit/0a9353a95e83b30b46b04047f06f359933306ec7))
 * **Uncategorized**
    - Release unleash-edge v1.0.0 ([`27c3df8`](https://github.com/Unleash/unleash-edge/commit/27c3df8c0609b7564d323b2af5c1df08841ce1d2))
    - clone value of cache entry ([`d3dfefc`](https://github.com/Unleash/unleash-edge/commit/d3dfefc08b4a2bdc837d153e89a17a5025908764))
</details>

## v0.5.1 (2023-03-15)

### Bug Fixes

 - <csr-id-c11ff4057398b63126effc93aa71578e328f79f4/> persist on shutdown also persists only validated tokens

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release unleash-edge v0.5.1 ([`4fb3ca9`](https://github.com/Unleash/unleash-edge/commit/4fb3ca98dc0c9c53ce1402e7048a0f3bee28f96c))
    - persist on shutdown also persists only validated tokens ([`c11ff40`](https://github.com/Unleash/unleash-edge/commit/c11ff4057398b63126effc93aa71578e328f79f4))
</details>

## v0.5.0 (2023-03-15)

<csr-id-51bcd7db417a43c29c756a096db24ec6eba5b1c4/>
<csr-id-510fe21aad1733ea8010637bd69fa0039c8e1400/>
<csr-id-eab611cd924a401dfd36e06670782f377d56cc81/>

### Chore

 - <csr-id-51bcd7db417a43c29c756a096db24ec6eba5b1c4/> adds a derive for TokenValidation status with a #[default] on the child enum

### Bug Fixes

 - <csr-id-dee24adaf6086c14b309160809211fad1a601899/> update rust crate serde to 1.0.156
 - <csr-id-796440a86ce4d734e73239adc55228b2cb39b059/> update rust-futures monorepo to 0.3.27
 - <csr-id-02005129a59847271b0cac09a9cd956601c33674/> update rust crate chrono to 0.4.24
 - <csr-id-7ba4b3a087fcf7165a2c02d6bd3c33ae037f0df8/> update rust crate serde to 1.0.155

### Other

 - <csr-id-510fe21aad1733ea8010637bd69fa0039c8e1400/> Prepare a token revalidator
   * task: Prepare a token revalidator
   
   Edge caching tokens could cause tokens to become out of sync with
   Unleash Upstream, adding a scheduled background task which ensures that
   our known tokens are still valid will go a long way towards mitigating
   this.
   
   * task: Make feature_refresher remove tokens/caches.
   
   When unleash_client receives forbidden from upstream, feature_refresher
   will now remove the token to refresh from its cache. If the token is the
   last one that resolves to a specific environment, it will also clean
   features_cache and engine_cache for that environment.
 - <csr-id-eab611cd924a401dfd36e06670782f377d56cc81/> Make Feature refresher register as client
   When a new token for registration comes in, feature refresher registers
   as a client upstream
   
   * chore: make register include actual refresh interval

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 8 commits contributed to the release over the course of 1 calendar day.
 - 6 days passed between releases.
 - 7 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 7 unique issues were worked on: [#102](https://github.com/Unleash/unleash-edge/issues/102), [#103](https://github.com/Unleash/unleash-edge/issues/103), [#105](https://github.com/Unleash/unleash-edge/issues/105), [#106](https://github.com/Unleash/unleash-edge/issues/106), [#107](https://github.com/Unleash/unleash-edge/issues/107), [#108](https://github.com/Unleash/unleash-edge/issues/108), [#109](https://github.com/Unleash/unleash-edge/issues/109)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#102](https://github.com/Unleash/unleash-edge/issues/102)**
    - Make Feature refresher register as client ([`eab611c`](https://github.com/Unleash/unleash-edge/commit/eab611cd924a401dfd36e06670782f377d56cc81))
 * **[#103](https://github.com/Unleash/unleash-edge/issues/103)**
    - adds a derive for TokenValidation status with a #[default] on the child enum ([`51bcd7d`](https://github.com/Unleash/unleash-edge/commit/51bcd7db417a43c29c756a096db24ec6eba5b1c4))
 * **[#105](https://github.com/Unleash/unleash-edge/issues/105)**
    - update rust-futures monorepo to 0.3.27 ([`796440a`](https://github.com/Unleash/unleash-edge/commit/796440a86ce4d734e73239adc55228b2cb39b059))
 * **[#106](https://github.com/Unleash/unleash-edge/issues/106)**
    - update rust crate serde to 1.0.155 ([`7ba4b3a`](https://github.com/Unleash/unleash-edge/commit/7ba4b3a087fcf7165a2c02d6bd3c33ae037f0df8))
 * **[#107](https://github.com/Unleash/unleash-edge/issues/107)**
    - update rust crate chrono to 0.4.24 ([`0200512`](https://github.com/Unleash/unleash-edge/commit/02005129a59847271b0cac09a9cd956601c33674))
 * **[#108](https://github.com/Unleash/unleash-edge/issues/108)**
    - update rust crate serde to 1.0.156 ([`dee24ad`](https://github.com/Unleash/unleash-edge/commit/dee24adaf6086c14b309160809211fad1a601899))
 * **[#109](https://github.com/Unleash/unleash-edge/issues/109)**
    - Prepare a token revalidator ([`510fe21`](https://github.com/Unleash/unleash-edge/commit/510fe21aad1733ea8010637bd69fa0039c8e1400))
 * **Uncategorized**
    - Release unleash-edge v0.5.0 ([`02d31d0`](https://github.com/Unleash/unleash-edge/commit/02d31d0325f36bd65aa33a3a3e21612b6af000fd))
</details>

## v0.4.1 (2023-03-09)

### Bug Fixes

 - <csr-id-8bd4e85740160dafcd185b4703fd4cb3db65f8c0/> make sure edgemode allows comma separated tokens for prewarming

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release unleash-edge v0.4.1 ([`68bce60`](https://github.com/Unleash/unleash-edge/commit/68bce604ee55631a8ccb11b49f8e75db3f45eb31))
    - make sure edgemode allows comma separated tokens for prewarming ([`8bd4e85`](https://github.com/Unleash/unleash-edge/commit/8bd4e85740160dafcd185b4703fd4cb3db65f8c0))
</details>

## v0.4.0 (2023-03-09)

<csr-id-f496004e73c6bce8ecf0485179a9bb1b25dca2fe/>

### Chore

 - <csr-id-f496004e73c6bce8ecf0485179a9bb1b25dca2fe/> update rust crate actix-http to 3.3.1

### Bug Fixes

 - <csr-id-1797ac70057328d32ed6cb7130fa720ccf659c63/> update rust crate serde_json to 1.0.94
 - <csr-id-15b1faa6680ef4f609ab16bb1caf54f6b7004091/> update rust crate serde to 1.0.154
 - <csr-id-34a945c402c2c0888b35e180c4a6ae3df3aa311f/> update rust crate async-trait to 0.1.66
 - <csr-id-a8a6a6afba5d696e3703eed79f167e2d3b5e3f62/> Move token cache resolution out of FromRequest
   * fix: Move token cache resolution out of FromRequest
* fix: metrics caches was passed in web::Data<Arc<MetricCache>>

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 6 commits contributed to the release.
 - 5 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 5 unique issues were worked on: [#100](https://github.com/Unleash/unleash-edge/issues/100), [#88](https://github.com/Unleash/unleash-edge/issues/88), [#90](https://github.com/Unleash/unleash-edge/issues/90), [#91](https://github.com/Unleash/unleash-edge/issues/91), [#97](https://github.com/Unleash/unleash-edge/issues/97)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#100](https://github.com/Unleash/unleash-edge/issues/100)**
    - Move token cache resolution out of FromRequest ([`a8a6a6a`](https://github.com/Unleash/unleash-edge/commit/a8a6a6afba5d696e3703eed79f167e2d3b5e3f62))
 * **[#88](https://github.com/Unleash/unleash-edge/issues/88)**
    - update rust crate actix-http to 3.3.1 ([`f496004`](https://github.com/Unleash/unleash-edge/commit/f496004e73c6bce8ecf0485179a9bb1b25dca2fe))
 * **[#90](https://github.com/Unleash/unleash-edge/issues/90)**
    - update rust crate async-trait to 0.1.66 ([`34a945c`](https://github.com/Unleash/unleash-edge/commit/34a945c402c2c0888b35e180c4a6ae3df3aa311f))
 * **[#91](https://github.com/Unleash/unleash-edge/issues/91)**
    - update rust crate serde_json to 1.0.94 ([`1797ac7`](https://github.com/Unleash/unleash-edge/commit/1797ac70057328d32ed6cb7130fa720ccf659c63))
 * **[#97](https://github.com/Unleash/unleash-edge/issues/97)**
    - update rust crate serde to 1.0.154 ([`15b1faa`](https://github.com/Unleash/unleash-edge/commit/15b1faa6680ef4f609ab16bb1caf54f6b7004091))
 * **Uncategorized**
    - Release unleash-edge v0.4.0 ([`c11fdce`](https://github.com/Unleash/unleash-edge/commit/c11fdce9e01f23a55ff6bb58c623f67be1792286))
</details>

## v0.3.2 (2023-03-08)

### Bug Fixes

 - <csr-id-5cd559399192a4381e117d3b190bb4f815a28817/> Uses token cache when resolving token from request

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#99](https://github.com/Unleash/unleash-edge/issues/99)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#99](https://github.com/Unleash/unleash-edge/issues/99)**
    - Uses token cache when resolving token from request ([`5cd5593`](https://github.com/Unleash/unleash-edge/commit/5cd559399192a4381e117d3b190bb4f815a28817))
 * **Uncategorized**
    - Release unleash-edge v0.3.2 ([`03dc7ca`](https://github.com/Unleash/unleash-edge/commit/03dc7caf177f7377670749ad82ce42c58ee0e0d6))
</details>

## v0.3.1 (2023-03-08)

<csr-id-036ab110d80cc696fcc166b3547294d2e0e6b6e1/>

### Chore

 - <csr-id-036ab110d80cc696fcc166b3547294d2e0e6b6e1/> Added tests for edge and client_api
   * fix: client_api endpoint wasn't filtering on which projects the token has access to.
   
   This also adds some Tarpaulin exclusion arguments to better reflect code
   coverage across code that we actually want to cover.
   
   There's still some work to do with regards to having the possibility to
   instantiate the entire application as a test service for full
   integration/e2e tests, but this at least covers more logic and also
   exposed a bug in how we validated keys when not using a token validator
   (offline mode))

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#98](https://github.com/Unleash/unleash-edge/issues/98)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#98](https://github.com/Unleash/unleash-edge/issues/98)**
    - Added tests for edge and client_api ([`036ab11`](https://github.com/Unleash/unleash-edge/commit/036ab110d80cc696fcc166b3547294d2e0e6b6e1))
 * **Uncategorized**
    - Release unleash-edge v0.3.1 ([`1eda8d6`](https://github.com/Unleash/unleash-edge/commit/1eda8d662ca9d991526a038985194f295a3fd74b))
</details>

## v0.3.0 (2023-03-07)

<csr-id-2fc9f70173970415e6995d1a2230699d7a2507a8/>

### Chore

 - <csr-id-2fc9f70173970415e6995d1a2230699d7a2507a8/> update pointers in README

### Documentation

 - <csr-id-c348c4f95ee8645a3ea1cdac03fb9bb338eae73d/> update release workflow

### New Features

 - <csr-id-a263dcaf0271ca38e83f7d55f5e62b4c699c148b/> lock free feature resolution
   Redesign the way data flows through Edge. Previously, we had thread locks on our data sources, which was impacting the response time of the application. This moves everything to be in memory cached with lazy persistence in the background and reloading the state on application startup. This means the hot path is now lock free.
   
   ---------

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 4 commits contributed to the release over the course of 6 calendar days.
 - 6 days passed between releases.
 - 3 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 1 unique issue was worked on: [#95](https://github.com/Unleash/unleash-edge/issues/95)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#95](https://github.com/Unleash/unleash-edge/issues/95)**
    - update release workflow ([`c348c4f`](https://github.com/Unleash/unleash-edge/commit/c348c4f95ee8645a3ea1cdac03fb9bb338eae73d))
 * **Uncategorized**
    - Release unleash-edge v0.3.0 ([`2e14660`](https://github.com/Unleash/unleash-edge/commit/2e146600a044d54c9db8610003607ae8b0872dd0))
    - lock free feature resolution ([`a263dca`](https://github.com/Unleash/unleash-edge/commit/a263dcaf0271ca38e83f7d55f5e62b4c699c148b))
    - update pointers in README ([`2fc9f70`](https://github.com/Unleash/unleash-edge/commit/2fc9f70173970415e6995d1a2230699d7a2507a8))
</details>

## v0.2.0 (2023-02-28)

<csr-id-176ef576d6ad6ddfb0993f7738465f2f68d3b4af/>
<csr-id-5875ebda52a75560800e4506e3a124016258a228/>

### Chore

 - <csr-id-176ef576d6ad6ddfb0993f7738465f2f68d3b4af/> bump shadow-rs to 0.21
 - <csr-id-5875ebda52a75560800e4506e3a124016258a228/> added symlink to top level README file

### New Features

 - <csr-id-60bcf7617b5673dbf66a345b4bed81857d65b70e/> Added /api/frontend endpoint to match Unleash

### Bug Fixes

 - <csr-id-ae3c9f75bcccddefd571d7fca4c87a7b4e585ea7/> add README to server subfolder
 - <csr-id-eaf0e797b57ec49ce5050826705d458798619a5b/> update rust crate clap to 4.1.8
 - <csr-id-2020281566c695f9e3e0a371f0bf9644613b2c38/> update rust crate actix-web to 4.3.1
 - <csr-id-3b6be69d527e73b7b23bcf2311df1099e0499e73/> update rust crate clap to 4.1.7
 - <csr-id-98666cf738ede56dd6ef5d7162194e2dafd1dcbb/> Move /api/client/register to a post request.
   Earlier we didn't accept metrics from downstream clients because we made
   a wrong assumption about Request Method type. This PR fixes this and
   starts accepting client metrics and posting them upstream.
 - <csr-id-77b9b0c3eb5a98b35224e16fd4594226be79cbb5/> Client features were not refreshing.
   We incorrectly assumed that our merge method would be enough here, but
   since the merge method retained the original and deduped, it seems like
   we tricked ourselves. The fix reduces the action to simply replacing
   whatever was cached with the newly fetched features from the server

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 11 commits contributed to the release over the course of 4 calendar days.
 - 4 days passed between releases.
 - 9 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 6 unique issues were worked on: [#76](https://github.com/Unleash/unleash-edge/issues/76), [#77](https://github.com/Unleash/unleash-edge/issues/77), [#78](https://github.com/Unleash/unleash-edge/issues/78), [#79](https://github.com/Unleash/unleash-edge/issues/79), [#81](https://github.com/Unleash/unleash-edge/issues/81), [#83](https://github.com/Unleash/unleash-edge/issues/83)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#76](https://github.com/Unleash/unleash-edge/issues/76)**
    - Added /api/frontend endpoint to match Unleash ([`60bcf76`](https://github.com/Unleash/unleash-edge/commit/60bcf7617b5673dbf66a345b4bed81857d65b70e))
 * **[#77](https://github.com/Unleash/unleash-edge/issues/77)**
    - update rust crate actix-web to 4.3.1 ([`2020281`](https://github.com/Unleash/unleash-edge/commit/2020281566c695f9e3e0a371f0bf9644613b2c38))
 * **[#78](https://github.com/Unleash/unleash-edge/issues/78)**
    - update rust crate clap to 4.1.7 ([`3b6be69`](https://github.com/Unleash/unleash-edge/commit/3b6be69d527e73b7b23bcf2311df1099e0499e73))
 * **[#79](https://github.com/Unleash/unleash-edge/issues/79)**
    - Client features were not refreshing. ([`77b9b0c`](https://github.com/Unleash/unleash-edge/commit/77b9b0c3eb5a98b35224e16fd4594226be79cbb5))
 * **[#81](https://github.com/Unleash/unleash-edge/issues/81)**
    - Move /api/client/register to a post request. ([`98666cf`](https://github.com/Unleash/unleash-edge/commit/98666cf738ede56dd6ef5d7162194e2dafd1dcbb))
 * **[#83](https://github.com/Unleash/unleash-edge/issues/83)**
    - update rust crate clap to 4.1.8 ([`eaf0e79`](https://github.com/Unleash/unleash-edge/commit/eaf0e797b57ec49ce5050826705d458798619a5b))
 * **Uncategorized**
    - Release unleash-edge v0.2.0 ([`f9735fd`](https://github.com/Unleash/unleash-edge/commit/f9735fd79a7ce9ba9bbc3848980dd561ea13c2ed))
    - Release unleash-edge v0.2.0 ([`a71fd76`](https://github.com/Unleash/unleash-edge/commit/a71fd7676c606eb9004fbfa15334f1de42a3d6f3))
    - add README to server subfolder ([`ae3c9f7`](https://github.com/Unleash/unleash-edge/commit/ae3c9f75bcccddefd571d7fca4c87a7b4e585ea7))
    - bump shadow-rs to 0.21 ([`176ef57`](https://github.com/Unleash/unleash-edge/commit/176ef576d6ad6ddfb0993f7738465f2f68d3b4af))
    - added symlink to top level README file ([`5875ebd`](https://github.com/Unleash/unleash-edge/commit/5875ebda52a75560800e4506e3a124016258a228))
</details>

## v0.1.1 (2023-02-24)

<csr-id-3f6920a5e56f3783594624eb370bff3af68ea91c/>
<csr-id-ffe24dcc7ec00097e43e5898b10373d6918aa234/>

### Chore

 - <csr-id-3f6920a5e56f3783594624eb370bff3af68ea91c/> remove rwlock from validator, client and builder
 - <csr-id-ffe24dcc7ec00097e43e5898b10373d6918aa234/> removal of RW locks for dashmaps

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release.
 - 1 day passed between releases.
 - 2 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#74](https://github.com/Unleash/unleash-edge/issues/74), [#75](https://github.com/Unleash/unleash-edge/issues/75)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#74](https://github.com/Unleash/unleash-edge/issues/74)**
    - removal of RW locks for dashmaps ([`ffe24dc`](https://github.com/Unleash/unleash-edge/commit/ffe24dcc7ec00097e43e5898b10373d6918aa234))
 * **[#75](https://github.com/Unleash/unleash-edge/issues/75)**
    - remove rwlock from validator, client and builder ([`3f6920a`](https://github.com/Unleash/unleash-edge/commit/3f6920a5e56f3783594624eb370bff3af68ea91c))
 * **Uncategorized**
    - Release unleash-edge v0.1.1 ([`ced1712`](https://github.com/Unleash/unleash-edge/commit/ced1712b186fc3cbad7dae1b061143234cd61c8f))
</details>

## v0.1.0 (2023-02-23)

<csr-id-cc123f6792494555c046a7eb6d164d066213c59d/>

### Chore

 - <csr-id-cc123f6792494555c046a7eb6d164d066213c59d/> update rust crate test-case to v3

### New Features

 - <csr-id-ab8e5ea52b8550ae97096f91d461f492dc9bd0d3/> allow controlling http server workers spun up
 - <csr-id-ac973797915b7d965721e77e3dba7a818033d87d/> implement metrics for front end clients

### Bug Fixes

 - <csr-id-aa2432e4efa9186bb5afa30df5dbc183d293672f/> update rust crate clap to 4.1.6
 - <csr-id-8ef7a33f61765cb7334d3791b64ffd0836bb0155/> Make offline mode handle non-Unleash tokens as valid secrets
 - <csr-id-b8b25d3075bafb83f3a14493a1dec0155835a2e9/> an issue where client features wouldn't correctly update in memory provider

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 8 commits contributed to the release over the course of 8 calendar days.
 - 8 days passed between releases.
 - 6 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 7 unique issues were worked on: [#63](https://github.com/Unleash/unleash-edge/issues/63), [#64](https://github.com/Unleash/unleash-edge/issues/64), [#65](https://github.com/Unleash/unleash-edge/issues/65), [#66](https://github.com/Unleash/unleash-edge/issues/66), [#67](https://github.com/Unleash/unleash-edge/issues/67), [#68](https://github.com/Unleash/unleash-edge/issues/68), [#72](https://github.com/Unleash/unleash-edge/issues/72)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#63](https://github.com/Unleash/unleash-edge/issues/63)**
    - update rust crate test-case to v3 ([`cc123f6`](https://github.com/Unleash/unleash-edge/commit/cc123f6792494555c046a7eb6d164d066213c59d))
 * **[#64](https://github.com/Unleash/unleash-edge/issues/64)**
    - an issue where client features wouldn't correctly update in memory provider ([`b8b25d3`](https://github.com/Unleash/unleash-edge/commit/b8b25d3075bafb83f3a14493a1dec0155835a2e9))
 * **[#65](https://github.com/Unleash/unleash-edge/issues/65)**
    - implement metrics for front end clients ([`ac97379`](https://github.com/Unleash/unleash-edge/commit/ac973797915b7d965721e77e3dba7a818033d87d))
 * **[#66](https://github.com/Unleash/unleash-edge/issues/66)**
    - allow controlling http server workers spun up ([`ab8e5ea`](https://github.com/Unleash/unleash-edge/commit/ab8e5ea52b8550ae97096f91d461f492dc9bd0d3))
 * **[#67](https://github.com/Unleash/unleash-edge/issues/67)**
    - Make offline mode handle non-Unleash tokens as valid secrets ([`8ef7a33`](https://github.com/Unleash/unleash-edge/commit/8ef7a33f61765cb7334d3791b64ffd0836bb0155))
 * **[#68](https://github.com/Unleash/unleash-edge/issues/68)**
    - update rust crate clap to 4.1.6 ([`aa2432e`](https://github.com/Unleash/unleash-edge/commit/aa2432e4efa9186bb5afa30df5dbc183d293672f))
 * **[#72](https://github.com/Unleash/unleash-edge/issues/72)**
    - Chore/data store refactor ([`026de50`](https://github.com/Unleash/unleash-edge/commit/026de501dabf9be3e9e8e001f0122452dc67dc22))
 * **Uncategorized**
    - Release unleash-edge v0.1.0 ([`6cbafd4`](https://github.com/Unleash/unleash-edge/commit/6cbafd4fcc6489ed26c1047bcdc5d7272c622800))
</details>

## v0.0.2 (2023-02-14)

### Bug Fixes

 - <csr-id-764e92e134a3074c0cb8ffe6376c638f165e3da8/> Use upstream_url rather than unleash_url

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Release unleash-edge v0.0.2 ([`11fd0bc`](https://github.com/Unleash/unleash-edge/commit/11fd0bcead4836d288dc4153acf842980f19ba5b))
    - Use upstream_url rather than unleash_url ([`764e92e`](https://github.com/Unleash/unleash-edge/commit/764e92e134a3074c0cb8ffe6376c638f165e3da8))
</details>

## v0.0.1 (2023-02-14)

<csr-id-004aa955e8bed7687090762efa0bcc53577ecf2c/>
<csr-id-b18c039255180c8d18e786e783a40f5cf9724358/>
<csr-id-869294b93591055b8b078943771915aef0bf33d8/>
<csr-id-9a34999914d7c27b01b2ab7793863c8c139589fd/>
<csr-id-cdfa7c216c1b7066ab059259d319a8c8ce2dc82a/>
<csr-id-ba72e090c400e7d2d7f276a89ecf79f3760c7c47/>
<csr-id-286dfd536ff1c5d865829dcd98bda49da6ad9d36/>
<csr-id-e58f4fc3306ae71c1bcb8e8704d38eeb176cac96/>
<csr-id-ea8cd1ba7fb36afb039f31ec4ba000a2b7271700/>
<csr-id-9132cc1410d1d4a14e08de15ee53c9fce1fc5c92/>
<csr-id-1d6a5188a6334b341db72f847f55450726da3bee/>
<csr-id-76e8e2a8d6e71bd1cf8920e00ce2373da9054a8e/>
<csr-id-45d6b6641c941e391a16df3294427efe64863c3c/>
<csr-id-749b3ad08de04644d0182d891e4f097dc0c438f5/>
<csr-id-d32e20bebc02fcc40670f508c86ab37ee8967b5f/>
<csr-id-bcc20510714f9c48985367e00fbd2eb6124e669a/>
<csr-id-b618ff1b1cd3ea30d2705b21db31be042d89309f/>
<csr-id-8f6fa05435caae5cdc112fefa187b8e0681df2dd/>
<csr-id-2d99d7e01e602185337f79529aba9f9fd86cd634/>
<csr-id-e2a589418c3bd305f04d3083b8ad1826e662956d/>

### Chore

 - <csr-id-004aa955e8bed7687090762efa0bcc53577ecf2c/> added team developer to save spam
 - <csr-id-b18c039255180c8d18e786e783a40f5cf9724358/> tokens are now used
 - <csr-id-869294b93591055b8b078943771915aef0bf33d8/> redesign source and sinks to store features by environment and filter the responses by project
 - <csr-id-9a34999914d7c27b01b2ab7793863c8c139589fd/> remove sinks for offline mode
 - <csr-id-cdfa7c216c1b7066ab059259d319a8c8ce2dc82a/> redesign source/sink architecture
 - <csr-id-ba72e090c400e7d2d7f276a89ecf79f3760c7c47/> remove redis test that doesn't make sense anymore
 - <csr-id-286dfd536ff1c5d865829dcd98bda49da6ad9d36/> test auto-assign-pr action
 - <csr-id-e58f4fc3306ae71c1bcb8e8704d38eeb176cac96/> move server startup and traits to async
 - <csr-id-ea8cd1ba7fb36afb039f31ec4ba000a2b7271700/> improve tests for redis provider
 - <csr-id-9132cc1410d1d4a14e08de15ee53c9fce1fc5c92/> bump unleash-types
 - <csr-id-1d6a5188a6334b341db72f847f55450726da3bee/> Update cargo keys with ownership and license

### Chore

 - <csr-id-e2a589418c3bd305f04d3083b8ad1826e662956d/> added changelog

### Documentation

<csr-id-16771118dbfdb4fc2dd819564b9d3f3355154134/>

 - <csr-id-e6fd6c5fda8adea94f06eaaf10033e9ae9a194a3/> add edge mode
   * docs: add edge mode
* docs: organize modes differently, small fixes
* docs: edge mode does not need token to start, explain warm up
* Update README.md

### New Features

<csr-id-92aa64bc58e4193adc95370e651579feddea2811/>
<csr-id-5f55517e4407a7acf4b7906d82eee737bb58a53d/>
<csr-id-8fe7cabbb496c34618cae77e82ddceeeb8cfb617/>
<csr-id-3addbd639c12749c5d18775f95b1bfede106c4cf/>
<csr-id-e6bc817c21affd7e06883a9d56f85f254878a4c8/>
<csr-id-4bf25a3402c8e9a3c48c63118da1469a69a3bbdd/>
<csr-id-c270685a08207e0ab283e563ad6f58ad4f859161/>
<csr-id-231efc30353f6af6f20b8431220101802ca5c2b3/>

 - <csr-id-3a8cd761a8cd92696c9229df1a6c3614aae261fa/> switch to backing with HashMap<TokenString, EdgeToken>
 - <csr-id-0d037ec243b120f093b5a20efb3c5ddda6e25767/> adds a call for validating tokens
 - <csr-id-eab0878ce2bf49a499f032a13c47f58a4b346cc7/> implement simplify tokens
 - <csr-id-9e99f4b64b3d53b2e79381a2cb0d80ef4b010b2b/> add client for getting features
 - <csr-id-5ae644c8e4c98c588111a7461f359439c994209f/> implement an in memory data store
 - <csr-id-0469918e24763a5fef41a706f6f88fde986f955d/> internal backstage build info endpoint
   * feat: internal backstage build info endpoint
* chore: add test documenting info endpoint
* feat: add enabled toggles routes
* fix: disabling metrics for not linux

### Bug Fixes

 - <csr-id-eea450a47bfe5c32ea84994570223c1d5a746bc8/> update rust crate unleash-types to 0.8.3
 - <csr-id-4f528b76b718405d151a06af6657376c9358a7a2/> update rust crate unleash-types to 0.8.2
 - <csr-id-2d4a74312db1e5adc0d042e52e47c4f7286a966d/> update rust crate unleash-yggdrasil to 0.4.5
 - <csr-id-986a7433f687de3126cf05bf8d776cabf3a28290/> update rust crate serde_json to 1.0.93
 - <csr-id-cd86cdd7c5f6a9a6577a10b01278e3b17e36811d/> update rust crate serde_json to 1.0.92
 - <csr-id-0be62e8547f76508f9f14f949958b8529ae96b39/> update rust crate anyhow to 1.0.69
 - <csr-id-ca0a50d711f8c504f2ad9671929abc663639264b/> expose correct route on frontend api
 - <csr-id-2b0f8320e4120b8451ddd004b8c83b1c8b9193bc/> features get refreshed.
   Previously our spin loop slept for 15 seconds and then hit the await on
   the channel for a new token to validate.
   This PR changes that to use tokio::select to either refresh features for
   known tokens each 10th second, or receive a new token to validate.
   
   Should allow us to use more than one token and get them refreshed
 - <csr-id-5593376c3a89b28df6b6a8be2c93c1dc38a30c89/> allow any on CORS
 - <csr-id-93b0f22802f3fb16ac97174ccf8dc2574dafb9e0/> make sure reqwest does not bring along openssl
 - <csr-id-46a10d229bf2ccfd03f367a8e34e6f7f9f148013/> update rust crate tokio to 1.25.0
 - <csr-id-be9428d76742a3f5b2436b8b5cb61374609b98c3/> update rust crate unleash-yggdrasil to 0.4.2
 - <csr-id-71a9a2372d2e5110b628fe30438cf5b6760c8899/> patch the way CORS headers are done, without this, the server crashes on startup with an unhelpful error message
 - <csr-id-4b9e889a3d42089f206b62b9eea45dcfd8bae2f3/> update rust crate clap to 4.1.4
 - <csr-id-02e201b5142e6b95ced38f3636d3015ce4f79e03/> Update unleash-types to 0.5.1
 - <csr-id-fa8e9610dc74dd6868e36cdb6d2ae46c3aa17303/> update rust crate unleash-yggdrasil to 0.4.0
 - <csr-id-9f817bd7f0039315ad40aa61319c6ff1543b5241/> update rust crate clap to 4.1.3
 - <csr-id-042ae381536614d76f387c8d24b82c9ed9cb93bc/> update rust crate actix-web to 4.3.0

### Other

 - <csr-id-76e8e2a8d6e71bd1cf8920e00ce2373da9054a8e/> move obvious debug level logging to debug
 - <csr-id-45d6b6641c941e391a16df3294427efe64863c3c/> Subsume keys to check
   This collapses the keys seen. Removing keys that have been subsumed by a
   wider key (a key that includes the same projects or more as existing
   keys).
 - <csr-id-749b3ad08de04644d0182d891e4f097dc0c438f5/> token validator
   * task: add token validator
 - <csr-id-d32e20bebc02fcc40670f508c86ab37ee8967b5f/> Updated to only refresh tokens of type Client
 - <csr-id-bcc20510714f9c48985367e00fbd2eb6124e669a/> update to include openapi and hashes feature of types
 - <csr-id-b618ff1b1cd3ea30d2705b21db31be042d89309f/> added etag middleware
 - <csr-id-8f6fa05435caae5cdc112fefa187b8e0681df2dd/> Added prometheus metrics from shadow

### Style

 - <csr-id-2d99d7e01e602185337f79529aba9f9fd86cd634/> fix formatting

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 59 commits contributed to the release over the course of 25 calendar days.
 - 54 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 47 unique issues were worked on: [#10](https://github.com/Unleash/unleash-edge/issues/10), [#12](https://github.com/Unleash/unleash-edge/issues/12), [#13](https://github.com/Unleash/unleash-edge/issues/13), [#14](https://github.com/Unleash/unleash-edge/issues/14), [#15](https://github.com/Unleash/unleash-edge/issues/15), [#16](https://github.com/Unleash/unleash-edge/issues/16), [#17](https://github.com/Unleash/unleash-edge/issues/17), [#18](https://github.com/Unleash/unleash-edge/issues/18), [#20](https://github.com/Unleash/unleash-edge/issues/20), [#22](https://github.com/Unleash/unleash-edge/issues/22), [#23](https://github.com/Unleash/unleash-edge/issues/23), [#25](https://github.com/Unleash/unleash-edge/issues/25), [#26](https://github.com/Unleash/unleash-edge/issues/26), [#27](https://github.com/Unleash/unleash-edge/issues/27), [#28](https://github.com/Unleash/unleash-edge/issues/28), [#29](https://github.com/Unleash/unleash-edge/issues/29), [#3](https://github.com/Unleash/unleash-edge/issues/3), [#30](https://github.com/Unleash/unleash-edge/issues/30), [#33](https://github.com/Unleash/unleash-edge/issues/33), [#34](https://github.com/Unleash/unleash-edge/issues/34), [#36](https://github.com/Unleash/unleash-edge/issues/36), [#37](https://github.com/Unleash/unleash-edge/issues/37), [#38](https://github.com/Unleash/unleash-edge/issues/38), [#39](https://github.com/Unleash/unleash-edge/issues/39), [#4](https://github.com/Unleash/unleash-edge/issues/4), [#40](https://github.com/Unleash/unleash-edge/issues/40), [#41](https://github.com/Unleash/unleash-edge/issues/41), [#42](https://github.com/Unleash/unleash-edge/issues/42), [#43](https://github.com/Unleash/unleash-edge/issues/43), [#44](https://github.com/Unleash/unleash-edge/issues/44), [#45](https://github.com/Unleash/unleash-edge/issues/45), [#46](https://github.com/Unleash/unleash-edge/issues/46), [#5](https://github.com/Unleash/unleash-edge/issues/5), [#52](https://github.com/Unleash/unleash-edge/issues/52), [#53](https://github.com/Unleash/unleash-edge/issues/53), [#54](https://github.com/Unleash/unleash-edge/issues/54), [#55](https://github.com/Unleash/unleash-edge/issues/55), [#56](https://github.com/Unleash/unleash-edge/issues/56), [#57](https://github.com/Unleash/unleash-edge/issues/57), [#58](https://github.com/Unleash/unleash-edge/issues/58), [#59](https://github.com/Unleash/unleash-edge/issues/59), [#6](https://github.com/Unleash/unleash-edge/issues/6), [#60](https://github.com/Unleash/unleash-edge/issues/60), [#61](https://github.com/Unleash/unleash-edge/issues/61), [#62](https://github.com/Unleash/unleash-edge/issues/62), [#8](https://github.com/Unleash/unleash-edge/issues/8), [#9](https://github.com/Unleash/unleash-edge/issues/9)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#10](https://github.com/Unleash/unleash-edge/issues/10)**
    - use subcommands rather than ValueEnum ([`8fe7cab`](https://github.com/Unleash/unleash-edge/commit/8fe7cabbb496c34618cae77e82ddceeeb8cfb617))
 * **[#12](https://github.com/Unleash/unleash-edge/issues/12)**
    - add basic proxy endpoints and related test code ([`5f55517`](https://github.com/Unleash/unleash-edge/commit/5f55517e4407a7acf4b7906d82eee737bb58a53d))
 * **[#13](https://github.com/Unleash/unleash-edge/issues/13)**
    - update rust crate clap to 4.1.4 ([`4b9e889`](https://github.com/Unleash/unleash-edge/commit/4b9e889a3d42089f206b62b9eea45dcfd8bae2f3))
 * **[#14](https://github.com/Unleash/unleash-edge/issues/14)**
    - patch the way CORS headers are done, without this, the server crashes on startup with an unhelpful error message ([`71a9a23`](https://github.com/Unleash/unleash-edge/commit/71a9a2372d2e5110b628fe30438cf5b6760c8899))
 * **[#15](https://github.com/Unleash/unleash-edge/issues/15)**
    - internal backstage build info endpoint ([`0469918`](https://github.com/Unleash/unleash-edge/commit/0469918e24763a5fef41a706f6f88fde986f955d))
 * **[#16](https://github.com/Unleash/unleash-edge/issues/16)**
    - add client for getting features ([`9e99f4b`](https://github.com/Unleash/unleash-edge/commit/9e99f4b64b3d53b2e79381a2cb0d80ef4b010b2b))
 * **[#17](https://github.com/Unleash/unleash-edge/issues/17)**
    - update rust crate unleash-yggdrasil to 0.4.2 ([`be9428d`](https://github.com/Unleash/unleash-edge/commit/be9428d76742a3f5b2436b8b5cb61374609b98c3))
 * **[#18](https://github.com/Unleash/unleash-edge/issues/18)**
    - add enabled toggles routes ([`92aa64b`](https://github.com/Unleash/unleash-edge/commit/92aa64bc58e4193adc95370e651579feddea2811))
 * **[#20](https://github.com/Unleash/unleash-edge/issues/20)**
    - Added prometheus metrics from shadow ([`8f6fa05`](https://github.com/Unleash/unleash-edge/commit/8f6fa05435caae5cdc112fefa187b8e0681df2dd))
 * **[#22](https://github.com/Unleash/unleash-edge/issues/22)**
    - added etag middleware ([`b618ff1`](https://github.com/Unleash/unleash-edge/commit/b618ff1b1cd3ea30d2705b21db31be042d89309f))
 * **[#23](https://github.com/Unleash/unleash-edge/issues/23)**
    - update rust crate tokio to 1.25.0 ([`46a10d2`](https://github.com/Unleash/unleash-edge/commit/46a10d229bf2ccfd03f367a8e34e6f7f9f148013))
 * **[#25](https://github.com/Unleash/unleash-edge/issues/25)**
    - Implement redis datasource ([`0b2537f`](https://github.com/Unleash/unleash-edge/commit/0b2537f4bd397c666d458589bf30f9322b0c9214))
 * **[#26](https://github.com/Unleash/unleash-edge/issues/26)**
    - update README ([`1677111`](https://github.com/Unleash/unleash-edge/commit/16771118dbfdb4fc2dd819564b9d3f3355154134))
 * **[#27](https://github.com/Unleash/unleash-edge/issues/27)**
    - fix formatting ([`2d99d7e`](https://github.com/Unleash/unleash-edge/commit/2d99d7e01e602185337f79529aba9f9fd86cd634))
 * **[#28](https://github.com/Unleash/unleash-edge/issues/28)**
    - improve tests for redis provider ([`ea8cd1b`](https://github.com/Unleash/unleash-edge/commit/ea8cd1ba7fb36afb039f31ec4ba000a2b7271700))
 * **[#29](https://github.com/Unleash/unleash-edge/issues/29)**
    - implement an in memory data store ([`5ae644c`](https://github.com/Unleash/unleash-edge/commit/5ae644c8e4c98c588111a7461f359439c994209f))
 * **[#3](https://github.com/Unleash/unleash-edge/issues/3)**
    - Adds client features endpoint ([`4bf25a3`](https://github.com/Unleash/unleash-edge/commit/4bf25a3402c8e9a3c48c63118da1469a69a3bbdd))
 * **[#30](https://github.com/Unleash/unleash-edge/issues/30)**
    - implement simplify tokens ([`eab0878`](https://github.com/Unleash/unleash-edge/commit/eab0878ce2bf49a499f032a13c47f58a4b346cc7))
 * **[#33](https://github.com/Unleash/unleash-edge/issues/33)**
    - move server startup and traits to async ([`e58f4fc`](https://github.com/Unleash/unleash-edge/commit/e58f4fc3306ae71c1bcb8e8704d38eeb176cac96))
 * **[#34](https://github.com/Unleash/unleash-edge/issues/34)**
    - adds a call for validating tokens ([`0d037ec`](https://github.com/Unleash/unleash-edge/commit/0d037ec243b120f093b5a20efb3c5ddda6e25767))
 * **[#36](https://github.com/Unleash/unleash-edge/issues/36)**
    - Feat/implement data sync ([`862ee28`](https://github.com/Unleash/unleash-edge/commit/862ee288eab20367c5d4e487ddd679f72174e8ef))
 * **[#37](https://github.com/Unleash/unleash-edge/issues/37)**
    - allow any on CORS ([`5593376`](https://github.com/Unleash/unleash-edge/commit/5593376c3a89b28df6b6a8be2c93c1dc38a30c89))
 * **[#38](https://github.com/Unleash/unleash-edge/issues/38)**
    - features get refreshed. ([`2b0f832`](https://github.com/Unleash/unleash-edge/commit/2b0f8320e4120b8451ddd004b8c83b1c8b9193bc))
 * **[#39](https://github.com/Unleash/unleash-edge/issues/39)**
    - test auto-assign-pr action ([`286dfd5`](https://github.com/Unleash/unleash-edge/commit/286dfd536ff1c5d865829dcd98bda49da6ad9d36))
 * **[#4](https://github.com/Unleash/unleash-edge/issues/4)**
    - Add edge-token extractor to lock down access ([`e6bc817`](https://github.com/Unleash/unleash-edge/commit/e6bc817c21affd7e06883a9d56f85f254878a4c8))
 * **[#40](https://github.com/Unleash/unleash-edge/issues/40)**
    - switch to backing with HashMap<TokenString, EdgeToken> ([`3a8cd76`](https://github.com/Unleash/unleash-edge/commit/3a8cd761a8cd92696c9229df1a6c3614aae261fa))
 * **[#41](https://github.com/Unleash/unleash-edge/issues/41)**
    - expose correct route on frontend api ([`ca0a50d`](https://github.com/Unleash/unleash-edge/commit/ca0a50d711f8c504f2ad9671929abc663639264b))
 * **[#42](https://github.com/Unleash/unleash-edge/issues/42)**
    - update rust crate anyhow to 1.0.69 ([`0be62e8`](https://github.com/Unleash/unleash-edge/commit/0be62e8547f76508f9f14f949958b8529ae96b39))
 * **[#43](https://github.com/Unleash/unleash-edge/issues/43)**
    - update rust crate serde_json to 1.0.92 ([`cd86cdd`](https://github.com/Unleash/unleash-edge/commit/cd86cdd7c5f6a9a6577a10b01278e3b17e36811d))
 * **[#44](https://github.com/Unleash/unleash-edge/issues/44)**
    - Updated to only refresh tokens of type Client ([`d32e20b`](https://github.com/Unleash/unleash-edge/commit/d32e20bebc02fcc40670f508c86ab37ee8967b5f))
 * **[#45](https://github.com/Unleash/unleash-edge/issues/45)**
    - remove redis test that doesn't make sense anymore ([`ba72e09`](https://github.com/Unleash/unleash-edge/commit/ba72e090c400e7d2d7f276a89ecf79f3760c7c47))
 * **[#46](https://github.com/Unleash/unleash-edge/issues/46)**
    - redesign source/sink architecture ([`cdfa7c2`](https://github.com/Unleash/unleash-edge/commit/cdfa7c216c1b7066ab059259d319a8c8ce2dc82a))
 * **[#5](https://github.com/Unleash/unleash-edge/issues/5)**
    - update rust crate actix-web to 4.3.0 ([`042ae38`](https://github.com/Unleash/unleash-edge/commit/042ae381536614d76f387c8d24b82c9ed9cb93bc))
 * **[#52](https://github.com/Unleash/unleash-edge/issues/52)**
    - update rust crate serde_json to 1.0.93 ([`986a743`](https://github.com/Unleash/unleash-edge/commit/986a7433f687de3126cf05bf8d776cabf3a28290))
 * **[#53](https://github.com/Unleash/unleash-edge/issues/53)**
    - Task client metrics ([`81d49ef`](https://github.com/Unleash/unleash-edge/commit/81d49ef4c360a168a5c7445e56bab7e2cc78c020))
 * **[#54](https://github.com/Unleash/unleash-edge/issues/54)**
    - remove sinks for offline mode ([`9a34999`](https://github.com/Unleash/unleash-edge/commit/9a34999914d7c27b01b2ab7793863c8c139589fd))
 * **[#55](https://github.com/Unleash/unleash-edge/issues/55)**
    - update rust crate unleash-types to 0.8.2 ([`4f528b7`](https://github.com/Unleash/unleash-edge/commit/4f528b76b718405d151a06af6657376c9358a7a2))
 * **[#56](https://github.com/Unleash/unleash-edge/issues/56)**
    - update rust crate unleash-yggdrasil to 0.4.5 ([`2d4a743`](https://github.com/Unleash/unleash-edge/commit/2d4a74312db1e5adc0d042e52e47c4f7286a966d))
 * **[#57](https://github.com/Unleash/unleash-edge/issues/57)**
    - redesign source and sinks to store features by environment and filter the responses by project ([`869294b`](https://github.com/Unleash/unleash-edge/commit/869294b93591055b8b078943771915aef0bf33d8))
 * **[#58](https://github.com/Unleash/unleash-edge/issues/58)**
    - token validator ([`749b3ad`](https://github.com/Unleash/unleash-edge/commit/749b3ad08de04644d0182d891e4f097dc0c438f5))
 * **[#59](https://github.com/Unleash/unleash-edge/issues/59)**
    - Subsume keys to check ([`45d6b66`](https://github.com/Unleash/unleash-edge/commit/45d6b6641c941e391a16df3294427efe64863c3c))
 * **[#6](https://github.com/Unleash/unleash-edge/issues/6)**
    - update rust crate clap to 4.1.3 ([`9f817bd`](https://github.com/Unleash/unleash-edge/commit/9f817bd7f0039315ad40aa61319c6ff1543b5241))
 * **[#60](https://github.com/Unleash/unleash-edge/issues/60)**
    - add edge mode ([`e6fd6c5`](https://github.com/Unleash/unleash-edge/commit/e6fd6c5fda8adea94f06eaaf10033e9ae9a194a3))
 * **[#61](https://github.com/Unleash/unleash-edge/issues/61)**
    - Open api docs ([`49d7129`](https://github.com/Unleash/unleash-edge/commit/49d7129a02f9ff8d9a336db9718593396742bb0d))
 * **[#62](https://github.com/Unleash/unleash-edge/issues/62)**
    - update rust crate unleash-types to 0.8.3 ([`eea450a`](https://github.com/Unleash/unleash-edge/commit/eea450a47bfe5c32ea84994570223c1d5a746bc8))
 * **[#8](https://github.com/Unleash/unleash-edge/issues/8)**
    - update rust crate unleash-yggdrasil to 0.4.0 ([`fa8e961`](https://github.com/Unleash/unleash-edge/commit/fa8e9610dc74dd6868e36cdb6d2ae46c3aa17303))
 * **[#9](https://github.com/Unleash/unleash-edge/issues/9)**
    - Added cors middleware ([`3addbd6`](https://github.com/Unleash/unleash-edge/commit/3addbd639c12749c5d18775f95b1bfede106c4cf))
 * **Uncategorized**
    - Release unleash-edge v0.0.1 ([`6187c4e`](https://github.com/Unleash/unleash-edge/commit/6187c4ef1fb79345e57bc8ac06efde2211e75798))
    - added changelog ([`e2a5894`](https://github.com/Unleash/unleash-edge/commit/e2a589418c3bd305f04d3083b8ad1826e662956d))
    - added team developer to save spam ([`004aa95`](https://github.com/Unleash/unleash-edge/commit/004aa955e8bed7687090762efa0bcc53577ecf2c))
    - move obvious debug level logging to debug ([`76e8e2a`](https://github.com/Unleash/unleash-edge/commit/76e8e2a8d6e71bd1cf8920e00ce2373da9054a8e))
    - tokens are now used ([`b18c039`](https://github.com/Unleash/unleash-edge/commit/b18c039255180c8d18e786e783a40f5cf9724358))
    - make sure reqwest does not bring along openssl ([`93b0f22`](https://github.com/Unleash/unleash-edge/commit/93b0f22802f3fb16ac97174ccf8dc2574dafb9e0))
    - update to include openapi and hashes feature of types ([`bcc2051`](https://github.com/Unleash/unleash-edge/commit/bcc20510714f9c48985367e00fbd2eb6124e669a))
    - bump unleash-types ([`9132cc1`](https://github.com/Unleash/unleash-edge/commit/9132cc1410d1d4a14e08de15ee53c9fce1fc5c92))
    - Update unleash-types to 0.5.1 ([`02e201b`](https://github.com/Unleash/unleash-edge/commit/02e201b5142e6b95ced38f3636d3015ce4f79e03))
    - Update cargo keys with ownership and license ([`1d6a518`](https://github.com/Unleash/unleash-edge/commit/1d6a5188a6334b341db72f847f55450726da3bee))
    - add /api/client/features endpoint ([`c270685`](https://github.com/Unleash/unleash-edge/commit/c270685a08207e0ab283e563ad6f58ad4f859161))
    - Server with metrics and health check ready ([`231efc3`](https://github.com/Unleash/unleash-edge/commit/231efc30353f6af6f20b8431220101802ca5c2b3))
</details>

