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
#### Troubleshooting: what if the version resolution doesn't work?

If the computed version isn't correct, you can run with the `--no-bump-on-demand` flag to make it work.

For instance, in this case, smart-release thought the next version should be 13.0.2 instead of 18.0.1:

> [INFO ] Manifest version of provided package 'unleash-edge' at 18.0.0 is sufficient to succeed latest released version 13.0.2, ignoring computed version 18.0.1

Running it with `--no-bump-on-demand` gives this output instead:

> [INFO ] WOULD patch-bump provided package 'unleash-edge' from 18.0.0 to 18.0.1 for publishing, 13.0.2 on crates.io

The doc string for `--no-bump-on-demand` is:
> --no-bump-on-demand              Always bump versions as specified by --bump or --bump-dependencies even if this is not required to publish a new version to crates.io

##### Why does this happen?

It appears that crates.io has changed it's index format and that smart-release hasn't quite been brought up to speed. 
### Perform the actual release when satisfied with output from the test
```sh
cargo smart-release -b <patch|minor|major> -u --execute
```

Remember to add the `--no-bump-on-demand` flag here too if it was needed to get the right version in the previous step.

This will display the changelog in your $EDITOR, once satisfied with changelog, quit out of the changelog preview, and the rest (building, publishing to cargo, tagging and making release notes) will be handled by smart-release

