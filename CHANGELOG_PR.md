
## [unleash-edge-v20.6.0] - 2026-09-04

### 🚀 Features
- expose context enrichers to public API (#1833) (by @sighphyre) - #1833

### 💼 Other
- bump taiki-e/install-action from 2.86.7 to 2.87.0 (#1823) (by @dependabot[bot]) - #1823
- bump taiki-e/install-action from 2.87.0 to 2.87.2 (#1835) (by @dependabot[bot]) - #1835
- bump quinn-proto from 0.11.14 to 0.11.16 (#1753) (by @dependabot[bot]) - #1753

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
