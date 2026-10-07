
## [unleash-edge-v20.6.0] - 2026-10-07

### 🚀 Features
- expose context enrichers to public API (#1833) (by @sighphyre) - #1833
- allow custom prom headers (#1898) (by @sighphyre) - #1898
- metrics for discarded feature state and failed background refreshes (#1755) (by @gastonfournier) - #1755

### 🐛 Bug Fixes
- make sure we allow startup even if Unleash can't be reached (#1875) (by @chriswk) - #1875
- only keep requested environments from HMAC/tokens (#1881) (by @chriswk) - #1881

### 💼 Other
- bump taiki-e/install-action from 2.86.7 to 2.87.0 (#1823) (by @dependabot[bot]) - #1823
- bump taiki-e/install-action from 2.87.0 to 2.87.2 (#1835) (by @dependabot[bot]) - #1835
- bump quinn-proto from 0.11.14 to 0.11.16 (#1753) (by @dependabot[bot]) - #1753
- bump taiki-e/install-action from 2.87.2 to 2.87.5 (#1845) (by @dependabot[bot]) - #1845
- bump taiki-e/install-action from 2.87.5 to 2.87.6 (#1847) (by @dependabot[bot]) - #1847
- bump taiki-e/install-action from 2.87.6 to 2.87.7 (#1852) (by @dependabot[bot]) - #1852
- bump ipnet from 2.12.1 to 2.12.2 (#1855) (by @dependabot[bot]) - #1855
- bump taiki-e/install-action from 2.87.7 to 2.87.8 (#1856) (by @dependabot[bot]) - #1856
- bump reqwest from 0.13.4 to 0.13.5 (#1862) (by @dependabot[bot]) - #1862
- bump taiki-e/install-action from 2.87.8 to 2.87.11 (#1863) (by @dependabot[bot]) - #1863
- bump taiki-e/install-action from 2.87.11 to 2.87.12 (#1866) (by @dependabot[bot]) - #1866
- bump p12-keystore from 0.3.1 to 0.3.2 (#1868) (by @dependabot[bot]) - #1868
- bump rustls from 0.23.43 to 0.23.45 (#1869) (by @dependabot[bot]) - #1869
- bump taiki-e/install-action from 2.87.12 to 2.87.13 (#1870) (by @dependabot[bot]) - #1870
- bump taiki-e/install-action from 2.87.13 to 2.87.16 (#1879) (by @dependabot[bot]) - #1879
- bump taiki-e/install-action from 2.87.16 to 2.87.17 (#1884) (by @dependabot[bot]) - #1884
- bump rand from 0.10.2 to 0.10.3 (#1886) (by @dependabot[bot]) - #1886
- bump taiki-e/install-action from 2.87.17 to 2.87.18 (#1888) (by @dependabot[bot]) - #1888
- bump taiki-e/install-action from 2.87.18 to 2.87.20 (#1891) (by @dependabot[bot]) - #1891
- bump taiki-e/install-action from 2.87.20 to 2.87.21 (#1893) (by @dependabot[bot]) - #1893
- bump hyper from 1.11.0 to 1.11.1 (#1830) (by @dependabot[bot]) - #1830
- bump redis from 1.6.0 to 1.7.0 (#1851) (by @dependabot[bot]) - #1851
- bump hyper-util from 0.1.20 to 0.1.21 (#1901) (by @dependabot[bot]) - #1901
- bump test-case from 3.3.1 to 3.4.0 (#1902) (by @dependabot[bot]) - #1902
- bump tokio-rustls from 0.26.4 to 0.26.6 (#1903) (by @dependabot[bot]) - #1903
- bump lazy_static from 1.5.0 to 1.5.1 (#1908) (by @dependabot[bot]) - #1908
- bump axum-test from 20.1.0 to 21.1.0 (#1811) (by @dependabot[bot]) - #1811
- bump tower-http from 0.7.0 to 0.7.1 (#1843) (by @dependabot[bot]) - #1843
- bump xxhash-rust from 0.8.15 to 0.8.19 (#1907) (by @dependabot[bot]) - #1907
- bump taiki-e/install-action from 2.87.21 to 2.87.23 (#1910) (by @dependabot[bot]) - #1910
- bump tokio from 1.53.1 to 1.53.2 (#1919) (by @dependabot[bot]) - #1919
- bump redis from 1.7.0 to 1.7.1 (#1918) (by @dependabot[bot]) - #1918

### 📚 Documentation
- simple enricher example (#1838) (by @sighphyre) - #1838
- more involved jwks example for context enrichers (#1840) (by @sighphyre) - #1840
- add a context enricher example that details how to assemble an artifact including external libraries (#1841) (by @sighphyre) - #1841

### ⚙️ Miscellaneous Tasks
- worker pool for enrichment (#1822) (by @sighphyre) - #1822
- initial wire up of context enrichers - still fully disabled (#1826) (by @sighphyre) - #1826
- context enrichers no longer take deep clones of headers on eve… (#1827) (by @sighphyre) - #1827
- don't clone context unnecessarily in context enricher flow (#1828) (by @sighphyre) - #1828
- propagate env vars to context enrichers (#1837) (by @sighphyre) - #1837
- update histogram for a little more granularity (#1842) (by @sighphyre) - #1842
- add ap-southeast-2 to replicate regions (#1859) (by @paulaguijarro) - #1859
- instance data sending for context enrichers (#1883) (by @sighphyre) - #1883
- make context enrichers work in docker release (#1895) (by @sighphyre) - #1895
- increase child timeout (#1912) (by @sighphyre) - #1912
- keep worker alive in a test to prevent a process race causing flakiness (#1913) (by @sighphyre) - #1913
- add cloudflare access headers to filebeat (#1916) (by @sastromskis) - #1916
