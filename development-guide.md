### Tools
* Install Rust using [rustup](https://rustup.rs)
* Copy the pre-commit hook in the hooks folder to .git/hooks/pre-commit
* Install [docker](https://docs.docker.com/get-docker), making sure your user has access to use it.

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
 - `cargo remove ...` - Remove a dependency from the Cargo.toml file
 - `cargo check` - Checks a local package and all of its dependencies for errors
 - `cargo clippy` - Run Clippy to get code warnings
 - `cargo fmt` - Format the code using rustfmt
 - `cargo test` - Run the tests
 - `cargo build` - Build a debug build. The executable will be available in ./target/debug/unleash-edge once successful
 - `cargo build --release` - Build a release build. The executable will be available in ./target/release/unleash-edge once successful. - If you want to run loadtesting, you should really build in release mode. 10-20x faster than the debug build
 - `cargo run -- edge --markdown-help -u http://localhost:4242 > CLI.md` - Update the CLI.md file.
 - `cargo run edge -h` - Start here to run edge mode locally.
 - `cargo run offline -h` - Start here to run offline mode locally.

### Testing

By default `cargo test` will run all the tests. If you want to exclude the expensive integration tests you can instead run `cargo test --bin unleash-edge`.

### Docker requirement
In order for all tests to successfully build, you'll need Docker installed. We use [testcontainers](https://github.com/testcontainers/testcontainers-rs) to spin up a redis container to test our redis feature. Testcontainers require docker to run