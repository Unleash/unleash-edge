# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Chore

 - <csr-id-9a651efc0393cebeb67e639aa612434606b4c9ed/> bump dependency status link

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

 - 5 commits contributed to the release over the course of 1 calendar day.
 - 1 day passed between releases.
 - 4 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 2 unique issues were worked on: [#151](https://github.com/Unleash/unleash-edge/issues/151), [#154](https://github.com/Unleash/unleash-edge/issues/154)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#151](https://github.com/Unleash/unleash-edge/issues/151)**
    - Added single feature evaluation endpoint ([`c417cab`](https://github.com/Unleash/unleash-edge/commit/c417cab5698ac1f45e8f640012b06b655abb900d))
 * **[#154](https://github.com/Unleash/unleash-edge/issues/154)**
    - Update rust crate serde_json to 1.0.96 ([`d1052f7`](https://github.com/Unleash/unleash-edge/commit/d1052f7f913713c488d3cd038709a872cb493c71))
 * **Uncategorized**
    - Updated README in server subfolder ([`625b076`](https://github.com/Unleash/unleash-edge/commit/625b0760c66574f94a098885ff94735330a2bb2d))
    - Bump dependency status link ([`9a651ef`](https://github.com/Unleash/unleash-edge/commit/9a651efc0393cebeb67e639aa612434606b4c9ed))
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
    - Update rust-futures monorepo to 0.3.28 ([`4899423`](https://github.com/Unleash/unleash-edge/commit/48994233364c9fa7af8dc331dede2da38913922e))
 * **[#147](https://github.com/Unleash/unleash-edge/issues/147)**
    - Add metrics endpoints for frontend ([`28fff02`](https://github.com/Unleash/unleash-edge/commit/28fff02aaf3bda6305186a323b4a507356bfd6db))
 * **[#149](https://github.com/Unleash/unleash-edge/issues/149)**
    - Add namePrefix filtering support ([`ebd6300`](https://github.com/Unleash/unleash-edge/commit/ebd63005c3ff2f73da7cb35872bd132d1c953dd7))
 * **Uncategorized**
    - Release unleash-edge v1.2.0 ([`ab51228`](https://github.com/Unleash/unleash-edge/commit/ab5122837f0476d055eaf007a55c13a715b1fdb3))
    - Update dependency links ([`26805bb`](https://github.com/Unleash/unleash-edge/commit/26805bb55b25edc4cc2e41f525c7eee71df4cd54))
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
    - Update rust crate clap to 4.2.0 ([`8e4df8d`](https://github.com/Unleash/unleash-edge/commit/8e4df8d5b6d8800ad644cac0c6cda7c19386426f))
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
    - Update rust crate reqwest to 0.11.15 ([`5034f87`](https://github.com/Unleash/unleash-edge/commit/5034f87f9d0d0d38bd8674fd00acc52bf863559a))
 * **[#121](https://github.com/Unleash/unleash-edge/issues/121)**
    - Update rust crate clap to 4.1.13 ([`f835db0`](https://github.com/Unleash/unleash-edge/commit/f835db09798cdd45181000b194348d7cd1f3ba08))
 * **[#122](https://github.com/Unleash/unleash-edge/issues/122)**
    - Post appropriately sized metric batches ([`b97681b`](https://github.com/Unleash/unleash-edge/commit/b97681b8e9d40afd35b629f0d9b4757c66a637a8))
 * **[#127](https://github.com/Unleash/unleash-edge/issues/127)**
    - Use fewer clones to reduce allocation ([`1ab5962`](https://github.com/Unleash/unleash-edge/commit/1ab5962ebc10c8a5f14492fcd28b46e541d2992d))
 * **[#135](https://github.com/Unleash/unleash-edge/issues/135)**
    - Added custom metrics handler to drop dependency ([`a858391`](https://github.com/Unleash/unleash-edge/commit/a858391e9cc7d9bd805a892519f38da6b4be0ebb))
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
    - Update rust crate clap to 4.1.9 ([`4d704b6`](https://github.com/Unleash/unleash-edge/commit/4d704b68b78eb066a03d0c5006979db3189f5d43))
 * **[#111](https://github.com/Unleash/unleash-edge/issues/111)**
    - Return 511 if edge has not hydrated. ([`584e61b`](https://github.com/Unleash/unleash-edge/commit/584e61bb98e32083996720f9d703341ca0025ed6))
 * **[#112](https://github.com/Unleash/unleash-edge/issues/112)**
    - Client features are hydrated synchronously. ([`1e73fdc`](https://github.com/Unleash/unleash-edge/commit/1e73fdcbce1786aea9f4a1b1a5a9a188c656e85c))
 * **[#113](https://github.com/Unleash/unleash-edge/issues/113)**
    - Update rust crate clap to 4.1.11 ([`b5604a3`](https://github.com/Unleash/unleash-edge/commit/b5604a34ee23aa17847fb8280c10cababce5ad26))
 * **[#116](https://github.com/Unleash/unleash-edge/issues/116)**
    - Update rust crate serde to 1.0.158 ([`0a9353a`](https://github.com/Unleash/unleash-edge/commit/0a9353a95e83b30b46b04047f06f359933306ec7))
 * **Uncategorized**
    - Release unleash-edge v1.0.0 ([`27c3df8`](https://github.com/Unleash/unleash-edge/commit/27c3df8c0609b7564d323b2af5c1df08841ce1d2))
    - Clone value of cache entry ([`d3dfefc`](https://github.com/Unleash/unleash-edge/commit/d3dfefc08b4a2bdc837d153e89a17a5025908764))
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
    - Persist on shutdown also persists only validated tokens ([`c11ff40`](https://github.com/Unleash/unleash-edge/commit/c11ff4057398b63126effc93aa71578e328f79f4))
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
    - Adds a derive for TokenValidation status with a #[default] on the child enum ([`51bcd7d`](https://github.com/Unleash/unleash-edge/commit/51bcd7db417a43c29c756a096db24ec6eba5b1c4))
 * **[#105](https://github.com/Unleash/unleash-edge/issues/105)**
    - Update rust-futures monorepo to 0.3.27 ([`796440a`](https://github.com/Unleash/unleash-edge/commit/796440a86ce4d734e73239adc55228b2cb39b059))
 * **[#106](https://github.com/Unleash/unleash-edge/issues/106)**
    - Update rust crate serde to 1.0.155 ([`7ba4b3a`](https://github.com/Unleash/unleash-edge/commit/7ba4b3a087fcf7165a2c02d6bd3c33ae037f0df8))
 * **[#107](https://github.com/Unleash/unleash-edge/issues/107)**
    - Update rust crate chrono to 0.4.24 ([`0200512`](https://github.com/Unleash/unleash-edge/commit/02005129a59847271b0cac09a9cd956601c33674))
 * **[#108](https://github.com/Unleash/unleash-edge/issues/108)**
    - Update rust crate serde to 1.0.156 ([`dee24ad`](https://github.com/Unleash/unleash-edge/commit/dee24adaf6086c14b309160809211fad1a601899))
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
    - Make sure edgemode allows comma separated tokens for prewarming ([`8bd4e85`](https://github.com/Unleash/unleash-edge/commit/8bd4e85740160dafcd185b4703fd4cb3db65f8c0))
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
    - Update rust crate actix-http to 3.3.1 ([`f496004`](https://github.com/Unleash/unleash-edge/commit/f496004e73c6bce8ecf0485179a9bb1b25dca2fe))
 * **[#90](https://github.com/Unleash/unleash-edge/issues/90)**
    - Update rust crate async-trait to 0.1.66 ([`34a945c`](https://github.com/Unleash/unleash-edge/commit/34a945c402c2c0888b35e180c4a6ae3df3aa311f))
 * **[#91](https://github.com/Unleash/unleash-edge/issues/91)**
    - Update rust crate serde_json to 1.0.94 ([`1797ac7`](https://github.com/Unleash/unleash-edge/commit/1797ac70057328d32ed6cb7130fa720ccf659c63))
 * **[#97](https://github.com/Unleash/unleash-edge/issues/97)**
    - Update rust crate serde to 1.0.154 ([`15b1faa`](https://github.com/Unleash/unleash-edge/commit/15b1faa6680ef4f609ab16bb1caf54f6b7004091))
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
    - Update release workflow ([`c348c4f`](https://github.com/Unleash/unleash-edge/commit/c348c4f95ee8645a3ea1cdac03fb9bb338eae73d))
 * **Uncategorized**
    - Release unleash-edge v0.3.0 ([`2e14660`](https://github.com/Unleash/unleash-edge/commit/2e146600a044d54c9db8610003607ae8b0872dd0))
    - Lock free feature resolution ([`a263dca`](https://github.com/Unleash/unleash-edge/commit/a263dcaf0271ca38e83f7d55f5e62b4c699c148b))
    - Update pointers in README ([`2fc9f70`](https://github.com/Unleash/unleash-edge/commit/2fc9f70173970415e6995d1a2230699d7a2507a8))
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
    - Update rust crate actix-web to 4.3.1 ([`2020281`](https://github.com/Unleash/unleash-edge/commit/2020281566c695f9e3e0a371f0bf9644613b2c38))
 * **[#78](https://github.com/Unleash/unleash-edge/issues/78)**
    - Update rust crate clap to 4.1.7 ([`3b6be69`](https://github.com/Unleash/unleash-edge/commit/3b6be69d527e73b7b23bcf2311df1099e0499e73))
 * **[#79](https://github.com/Unleash/unleash-edge/issues/79)**
    - Client features were not refreshing. ([`77b9b0c`](https://github.com/Unleash/unleash-edge/commit/77b9b0c3eb5a98b35224e16fd4594226be79cbb5))
 * **[#81](https://github.com/Unleash/unleash-edge/issues/81)**
    - Move /api/client/register to a post request. ([`98666cf`](https://github.com/Unleash/unleash-edge/commit/98666cf738ede56dd6ef5d7162194e2dafd1dcbb))
 * **[#83](https://github.com/Unleash/unleash-edge/issues/83)**
    - Update rust crate clap to 4.1.8 ([`eaf0e79`](https://github.com/Unleash/unleash-edge/commit/eaf0e797b57ec49ce5050826705d458798619a5b))
 * **Uncategorized**
    - Release unleash-edge v0.2.0 ([`f9735fd`](https://github.com/Unleash/unleash-edge/commit/f9735fd79a7ce9ba9bbc3848980dd561ea13c2ed))
    - Release unleash-edge v0.2.0 ([`a71fd76`](https://github.com/Unleash/unleash-edge/commit/a71fd7676c606eb9004fbfa15334f1de42a3d6f3))
    - Add README to server subfolder ([`ae3c9f7`](https://github.com/Unleash/unleash-edge/commit/ae3c9f75bcccddefd571d7fca4c87a7b4e585ea7))
    - Bump shadow-rs to 0.21 ([`176ef57`](https://github.com/Unleash/unleash-edge/commit/176ef576d6ad6ddfb0993f7738465f2f68d3b4af))
    - Added symlink to top level README file ([`5875ebd`](https://github.com/Unleash/unleash-edge/commit/5875ebda52a75560800e4506e3a124016258a228))
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
    - Removal of RW locks for dashmaps ([`ffe24dc`](https://github.com/Unleash/unleash-edge/commit/ffe24dcc7ec00097e43e5898b10373d6918aa234))
 * **[#75](https://github.com/Unleash/unleash-edge/issues/75)**
    - Remove rwlock from validator, client and builder ([`3f6920a`](https://github.com/Unleash/unleash-edge/commit/3f6920a5e56f3783594624eb370bff3af68ea91c))
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
    - Update rust crate test-case to v3 ([`cc123f6`](https://github.com/Unleash/unleash-edge/commit/cc123f6792494555c046a7eb6d164d066213c59d))
 * **[#64](https://github.com/Unleash/unleash-edge/issues/64)**
    - An issue where client features wouldn't correctly update in memory provider ([`b8b25d3`](https://github.com/Unleash/unleash-edge/commit/b8b25d3075bafb83f3a14493a1dec0155835a2e9))
 * **[#65](https://github.com/Unleash/unleash-edge/issues/65)**
    - Implement metrics for front end clients ([`ac97379`](https://github.com/Unleash/unleash-edge/commit/ac973797915b7d965721e77e3dba7a818033d87d))
 * **[#66](https://github.com/Unleash/unleash-edge/issues/66)**
    - Allow controlling http server workers spun up ([`ab8e5ea`](https://github.com/Unleash/unleash-edge/commit/ab8e5ea52b8550ae97096f91d461f492dc9bd0d3))
 * **[#67](https://github.com/Unleash/unleash-edge/issues/67)**
    - Make offline mode handle non-Unleash tokens as valid secrets ([`8ef7a33`](https://github.com/Unleash/unleash-edge/commit/8ef7a33f61765cb7334d3791b64ffd0836bb0155))
 * **[#68](https://github.com/Unleash/unleash-edge/issues/68)**
    - Update rust crate clap to 4.1.6 ([`aa2432e`](https://github.com/Unleash/unleash-edge/commit/aa2432e4efa9186bb5afa30df5dbc183d293672f))
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
    - Use subcommands rather than ValueEnum ([`8fe7cab`](https://github.com/Unleash/unleash-edge/commit/8fe7cabbb496c34618cae77e82ddceeeb8cfb617))
 * **[#12](https://github.com/Unleash/unleash-edge/issues/12)**
    - Add basic proxy endpoints and related test code ([`5f55517`](https://github.com/Unleash/unleash-edge/commit/5f55517e4407a7acf4b7906d82eee737bb58a53d))
 * **[#13](https://github.com/Unleash/unleash-edge/issues/13)**
    - Update rust crate clap to 4.1.4 ([`4b9e889`](https://github.com/Unleash/unleash-edge/commit/4b9e889a3d42089f206b62b9eea45dcfd8bae2f3))
 * **[#14](https://github.com/Unleash/unleash-edge/issues/14)**
    - Patch the way CORS headers are done, without this, the server crashes on startup with an unhelpful error message ([`71a9a23`](https://github.com/Unleash/unleash-edge/commit/71a9a2372d2e5110b628fe30438cf5b6760c8899))
 * **[#15](https://github.com/Unleash/unleash-edge/issues/15)**
    - Internal backstage build info endpoint ([`0469918`](https://github.com/Unleash/unleash-edge/commit/0469918e24763a5fef41a706f6f88fde986f955d))
 * **[#16](https://github.com/Unleash/unleash-edge/issues/16)**
    - Add client for getting features ([`9e99f4b`](https://github.com/Unleash/unleash-edge/commit/9e99f4b64b3d53b2e79381a2cb0d80ef4b010b2b))
 * **[#17](https://github.com/Unleash/unleash-edge/issues/17)**
    - Update rust crate unleash-yggdrasil to 0.4.2 ([`be9428d`](https://github.com/Unleash/unleash-edge/commit/be9428d76742a3f5b2436b8b5cb61374609b98c3))
 * **[#18](https://github.com/Unleash/unleash-edge/issues/18)**
    - Add enabled toggles routes ([`92aa64b`](https://github.com/Unleash/unleash-edge/commit/92aa64bc58e4193adc95370e651579feddea2811))
 * **[#20](https://github.com/Unleash/unleash-edge/issues/20)**
    - Added prometheus metrics from shadow ([`8f6fa05`](https://github.com/Unleash/unleash-edge/commit/8f6fa05435caae5cdc112fefa187b8e0681df2dd))
 * **[#22](https://github.com/Unleash/unleash-edge/issues/22)**
    - Added etag middleware ([`b618ff1`](https://github.com/Unleash/unleash-edge/commit/b618ff1b1cd3ea30d2705b21db31be042d89309f))
 * **[#23](https://github.com/Unleash/unleash-edge/issues/23)**
    - Update rust crate tokio to 1.25.0 ([`46a10d2`](https://github.com/Unleash/unleash-edge/commit/46a10d229bf2ccfd03f367a8e34e6f7f9f148013))
 * **[#25](https://github.com/Unleash/unleash-edge/issues/25)**
    - Implement redis datasource ([`0b2537f`](https://github.com/Unleash/unleash-edge/commit/0b2537f4bd397c666d458589bf30f9322b0c9214))
 * **[#26](https://github.com/Unleash/unleash-edge/issues/26)**
    - Update README ([`1677111`](https://github.com/Unleash/unleash-edge/commit/16771118dbfdb4fc2dd819564b9d3f3355154134))
 * **[#27](https://github.com/Unleash/unleash-edge/issues/27)**
    - Fix formatting ([`2d99d7e`](https://github.com/Unleash/unleash-edge/commit/2d99d7e01e602185337f79529aba9f9fd86cd634))
 * **[#28](https://github.com/Unleash/unleash-edge/issues/28)**
    - Improve tests for redis provider ([`ea8cd1b`](https://github.com/Unleash/unleash-edge/commit/ea8cd1ba7fb36afb039f31ec4ba000a2b7271700))
 * **[#29](https://github.com/Unleash/unleash-edge/issues/29)**
    - Implement an in memory data store ([`5ae644c`](https://github.com/Unleash/unleash-edge/commit/5ae644c8e4c98c588111a7461f359439c994209f))
 * **[#3](https://github.com/Unleash/unleash-edge/issues/3)**
    - Adds client features endpoint ([`4bf25a3`](https://github.com/Unleash/unleash-edge/commit/4bf25a3402c8e9a3c48c63118da1469a69a3bbdd))
 * **[#30](https://github.com/Unleash/unleash-edge/issues/30)**
    - Implement simplify tokens ([`eab0878`](https://github.com/Unleash/unleash-edge/commit/eab0878ce2bf49a499f032a13c47f58a4b346cc7))
 * **[#33](https://github.com/Unleash/unleash-edge/issues/33)**
    - Move server startup and traits to async ([`e58f4fc`](https://github.com/Unleash/unleash-edge/commit/e58f4fc3306ae71c1bcb8e8704d38eeb176cac96))
 * **[#34](https://github.com/Unleash/unleash-edge/issues/34)**
    - Adds a call for validating tokens ([`0d037ec`](https://github.com/Unleash/unleash-edge/commit/0d037ec243b120f093b5a20efb3c5ddda6e25767))
 * **[#36](https://github.com/Unleash/unleash-edge/issues/36)**
    - Feat/implement data sync ([`862ee28`](https://github.com/Unleash/unleash-edge/commit/862ee288eab20367c5d4e487ddd679f72174e8ef))
 * **[#37](https://github.com/Unleash/unleash-edge/issues/37)**
    - Allow any on CORS ([`5593376`](https://github.com/Unleash/unleash-edge/commit/5593376c3a89b28df6b6a8be2c93c1dc38a30c89))
 * **[#38](https://github.com/Unleash/unleash-edge/issues/38)**
    - Features get refreshed. ([`2b0f832`](https://github.com/Unleash/unleash-edge/commit/2b0f8320e4120b8451ddd004b8c83b1c8b9193bc))
 * **[#39](https://github.com/Unleash/unleash-edge/issues/39)**
    - Test auto-assign-pr action ([`286dfd5`](https://github.com/Unleash/unleash-edge/commit/286dfd536ff1c5d865829dcd98bda49da6ad9d36))
 * **[#4](https://github.com/Unleash/unleash-edge/issues/4)**
    - Add edge-token extractor to lock down access ([`e6bc817`](https://github.com/Unleash/unleash-edge/commit/e6bc817c21affd7e06883a9d56f85f254878a4c8))
 * **[#40](https://github.com/Unleash/unleash-edge/issues/40)**
    - Switch to backing with HashMap<TokenString, EdgeToken> ([`3a8cd76`](https://github.com/Unleash/unleash-edge/commit/3a8cd761a8cd92696c9229df1a6c3614aae261fa))
 * **[#41](https://github.com/Unleash/unleash-edge/issues/41)**
    - Expose correct route on frontend api ([`ca0a50d`](https://github.com/Unleash/unleash-edge/commit/ca0a50d711f8c504f2ad9671929abc663639264b))
 * **[#42](https://github.com/Unleash/unleash-edge/issues/42)**
    - Update rust crate anyhow to 1.0.69 ([`0be62e8`](https://github.com/Unleash/unleash-edge/commit/0be62e8547f76508f9f14f949958b8529ae96b39))
 * **[#43](https://github.com/Unleash/unleash-edge/issues/43)**
    - Update rust crate serde_json to 1.0.92 ([`cd86cdd`](https://github.com/Unleash/unleash-edge/commit/cd86cdd7c5f6a9a6577a10b01278e3b17e36811d))
 * **[#44](https://github.com/Unleash/unleash-edge/issues/44)**
    - Updated to only refresh tokens of type Client ([`d32e20b`](https://github.com/Unleash/unleash-edge/commit/d32e20bebc02fcc40670f508c86ab37ee8967b5f))
 * **[#45](https://github.com/Unleash/unleash-edge/issues/45)**
    - Remove redis test that doesn't make sense anymore ([`ba72e09`](https://github.com/Unleash/unleash-edge/commit/ba72e090c400e7d2d7f276a89ecf79f3760c7c47))
 * **[#46](https://github.com/Unleash/unleash-edge/issues/46)**
    - Redesign source/sink architecture ([`cdfa7c2`](https://github.com/Unleash/unleash-edge/commit/cdfa7c216c1b7066ab059259d319a8c8ce2dc82a))
 * **[#5](https://github.com/Unleash/unleash-edge/issues/5)**
    - Update rust crate actix-web to 4.3.0 ([`042ae38`](https://github.com/Unleash/unleash-edge/commit/042ae381536614d76f387c8d24b82c9ed9cb93bc))
 * **[#52](https://github.com/Unleash/unleash-edge/issues/52)**
    - Update rust crate serde_json to 1.0.93 ([`986a743`](https://github.com/Unleash/unleash-edge/commit/986a7433f687de3126cf05bf8d776cabf3a28290))
 * **[#53](https://github.com/Unleash/unleash-edge/issues/53)**
    - Task client metrics ([`81d49ef`](https://github.com/Unleash/unleash-edge/commit/81d49ef4c360a168a5c7445e56bab7e2cc78c020))
 * **[#54](https://github.com/Unleash/unleash-edge/issues/54)**
    - Remove sinks for offline mode ([`9a34999`](https://github.com/Unleash/unleash-edge/commit/9a34999914d7c27b01b2ab7793863c8c139589fd))
 * **[#55](https://github.com/Unleash/unleash-edge/issues/55)**
    - Update rust crate unleash-types to 0.8.2 ([`4f528b7`](https://github.com/Unleash/unleash-edge/commit/4f528b76b718405d151a06af6657376c9358a7a2))
 * **[#56](https://github.com/Unleash/unleash-edge/issues/56)**
    - Update rust crate unleash-yggdrasil to 0.4.5 ([`2d4a743`](https://github.com/Unleash/unleash-edge/commit/2d4a74312db1e5adc0d042e52e47c4f7286a966d))
 * **[#57](https://github.com/Unleash/unleash-edge/issues/57)**
    - Redesign source and sinks to store features by environment and filter the responses by project ([`869294b`](https://github.com/Unleash/unleash-edge/commit/869294b93591055b8b078943771915aef0bf33d8))
 * **[#58](https://github.com/Unleash/unleash-edge/issues/58)**
    - Token validator ([`749b3ad`](https://github.com/Unleash/unleash-edge/commit/749b3ad08de04644d0182d891e4f097dc0c438f5))
 * **[#59](https://github.com/Unleash/unleash-edge/issues/59)**
    - Subsume keys to check ([`45d6b66`](https://github.com/Unleash/unleash-edge/commit/45d6b6641c941e391a16df3294427efe64863c3c))
 * **[#6](https://github.com/Unleash/unleash-edge/issues/6)**
    - Update rust crate clap to 4.1.3 ([`9f817bd`](https://github.com/Unleash/unleash-edge/commit/9f817bd7f0039315ad40aa61319c6ff1543b5241))
 * **[#60](https://github.com/Unleash/unleash-edge/issues/60)**
    - Add edge mode ([`e6fd6c5`](https://github.com/Unleash/unleash-edge/commit/e6fd6c5fda8adea94f06eaaf10033e9ae9a194a3))
 * **[#61](https://github.com/Unleash/unleash-edge/issues/61)**
    - Open api docs ([`49d7129`](https://github.com/Unleash/unleash-edge/commit/49d7129a02f9ff8d9a336db9718593396742bb0d))
 * **[#62](https://github.com/Unleash/unleash-edge/issues/62)**
    - Update rust crate unleash-types to 0.8.3 ([`eea450a`](https://github.com/Unleash/unleash-edge/commit/eea450a47bfe5c32ea84994570223c1d5a746bc8))
 * **[#8](https://github.com/Unleash/unleash-edge/issues/8)**
    - Update rust crate unleash-yggdrasil to 0.4.0 ([`fa8e961`](https://github.com/Unleash/unleash-edge/commit/fa8e9610dc74dd6868e36cdb6d2ae46c3aa17303))
 * **[#9](https://github.com/Unleash/unleash-edge/issues/9)**
    - Added cors middleware ([`3addbd6`](https://github.com/Unleash/unleash-edge/commit/3addbd639c12749c5d18775f95b1bfede106c4cf))
 * **Uncategorized**
    - Release unleash-edge v0.0.1 ([`6187c4e`](https://github.com/Unleash/unleash-edge/commit/6187c4ef1fb79345e57bc8ac06efde2211e75798))
    - Added changelog ([`e2a5894`](https://github.com/Unleash/unleash-edge/commit/e2a589418c3bd305f04d3083b8ad1826e662956d))
    - Added team developer to save spam ([`004aa95`](https://github.com/Unleash/unleash-edge/commit/004aa955e8bed7687090762efa0bcc53577ecf2c))
    - Move obvious debug level logging to debug ([`76e8e2a`](https://github.com/Unleash/unleash-edge/commit/76e8e2a8d6e71bd1cf8920e00ce2373da9054a8e))
    - Tokens are now used ([`b18c039`](https://github.com/Unleash/unleash-edge/commit/b18c039255180c8d18e786e783a40f5cf9724358))
    - Make sure reqwest does not bring along openssl ([`93b0f22`](https://github.com/Unleash/unleash-edge/commit/93b0f22802f3fb16ac97174ccf8dc2574dafb9e0))
    - Update to include openapi and hashes feature of types ([`bcc2051`](https://github.com/Unleash/unleash-edge/commit/bcc20510714f9c48985367e00fbd2eb6124e669a))
    - Bump unleash-types ([`9132cc1`](https://github.com/Unleash/unleash-edge/commit/9132cc1410d1d4a14e08de15ee53c9fce1fc5c92))
    - Update unleash-types to 0.5.1 ([`02e201b`](https://github.com/Unleash/unleash-edge/commit/02e201b5142e6b95ced38f3636d3015ce4f79e03))
    - Update cargo keys with ownership and license ([`1d6a518`](https://github.com/Unleash/unleash-edge/commit/1d6a5188a6334b341db72f847f55450726da3bee))
    - Add /api/client/features endpoint ([`c270685`](https://github.com/Unleash/unleash-edge/commit/c270685a08207e0ab283e563ad6f58ad4f859161))
    - Server with metrics and health check ready ([`231efc3`](https://github.com/Unleash/unleash-edge/commit/231efc30353f6af6f20b8431220101802ca5c2b3))
</details>

