### Tools
* Install Rust using [rustup](https://rustup.rs)
* Copy the pre-commit hook in the hooks folder into .git/hooks/pre-commit

```shell
cp hooks/* .git/hooks/
```

* For test coverage: `cargo install cargo-tarpaulin`
* For smart release: `cargo install cargo-smart-release`
* For workspaces: `cargo install cargo-workspaces`

### Practices
* We use rustfmt (triggered from cargo fmt) to format our source code
* We try to keep to [conventional-commits](https://www.conventionalcommits.org/en/v1.0.0) - This simplifies creating and keeping a changelog up to date as well as helping to decide which version to bump when releasing a new version
* We will adhere to [Semantic versioning](https://semver.org/)
* We use Clippy to ensure that our code follows a consistent format, you can run Clippy using `cargo clippy --fix` once it's installed

### Common commands

 - `cargo add ...` - Add a dependency to the Cargo.toml file


### Testing

By default `cargo test` will run all the tests. If you want to exclude the expensive integration tests you can instead run `cargo test --bin unleash-edge`.