## Releasing Unleash Edge

This project uses [release-plz](https://release-plz.dev/) to release. Releasing is a matter of merging the "Release {new
version}" PR from Github's interface.

### Semantic versioning

Release-plz uses [next_version](https://docs.rs/next_version/latest/next_version/) to decide what type of release to
make, so if it's not making the PR for the type of release you expect, consult the docs first.
