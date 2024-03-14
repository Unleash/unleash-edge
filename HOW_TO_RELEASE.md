## Releasing Unleash Edge
This project uses cargo-smart-release and the github cli to perform releases.

### Installing the necessary tools
#### Cargo-smart-release

```sh
cargo install cargo-smart-release
```

#### Github CLI
Visit https://cli.github.com/ and follow the instructions for your platform

### Configuring the necessary tools

#### Cargo
You need to be logged in to crates with a user with publishing rights.
```
cargo login
```

#### Github CLI
You need to be logged in with a user allowed to make tags and releases.


## Releasing

### Update links to dependency graph
We do this before the release, so the README on crates points to the dependencies for the actual released version.
```sh
sed -i s/<current_version>/<next_version>/g README.md
sed -i s/<current_version>/<next_version>/g server/README.md
```

### If you've changed CLI arguments
To update CLI.md we've built in a task to the edge binary which prints the Clap help file to markdown format. Run
```sh
cargo run -- edge -u http://localhost:4242 --markdown-help > CLI.md
```

### Commit the updated files
```sh
git commit -m"chore: Prepare for release"
```

### Test that version resolution works
```sh
cargo smart-release -b <patch|minor|major> -u
```

### Perform the actual release when satisfied with output from the test
```sh
cargo smart-release -b <patch|minor|major> -u --execute
```
This will display the changelog in your $EDITOR, once satisfied with changelog, quit out of the changelog preview, and the rest (building, publishing to cargo, tagging and making release notes) will be handled by smart-release

