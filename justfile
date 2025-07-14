default:
    just test

build:
    CRANELIFT_BUILD=1 RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
    cargo +nightly build --no-default-features

test:
    CRANELIFT_BUILD=1 RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
    cargo +nightly test --lib --no-default-features
