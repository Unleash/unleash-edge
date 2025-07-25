default:
    just test

build:
    CRANELIFT_BUILD=1 RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
    cargo +nightly build --no-default-features

test *ARGS:
    CRANELIFT_BUILD=1 RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
    cargo +nightly test --lib --no-default-features -- {{ARGS}}

run *ARGS:
    CRANELIFT_BUILD=1 RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
    cargo +nightly run --no-default-features -- {{ARGS}}

sysbuild:
    CRANELIFT_BUILD=1 cargo +nightly build --no-default-features

systest *ARGS:
    CRANELIFT_BUILD=1 cargo +nightly test --lib --no-default-features -- --test-threads=1 {{ARGS}}
