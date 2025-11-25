FROM --platform=linux/amd64 lukemathwalker/cargo-chef:latest-rust-latest AS amd64-chef
FROM --platform=linux/arm64 lukemathwalker/cargo-chef:latest-rust-latest AS arm64-chef

# Base image for the build stage - this is a multi-stage build that uses cross-compilation (thanks to --platform switch)
FROM --platform=$BUILDPLATFORM lukemathwalker/cargo-chef:latest-rust-latest AS chef
WORKDIR /app

# Planner stage
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Builder stage
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

ARG TARGETPLATFORM
ARG TARGETARCH
ARG CARGO_FEATURES=""
ENV CARGO_FEATURES=${CARGO_FEATURES}

# Copy runtime dependencies for specific target platform/architecture
# ARM specific folders
WORKDIR /all-files/linux/arm64/lib/aarch64-linux-gnu

# AMD64 specific folders
WORKDIR /all-files/linux/amd64/lib/x86_64-linux-gnu
WORKDIR /all-files/linux/amd64/lib64

# Common folders
WORKDIR /all-files/${TARGETPLATFORM}/etc/ssl/certs
WORKDIR /all-files/${TARGETPLATFORM}/app

# ARM64
COPY --from=arm64-chef \
    /lib/aarch64-linux-gnu/libgcc_s.so.1 \
    /lib/aarch64-linux-gnu/libm.so.6 \
    /lib/aarch64-linux-gnu/libc.so.6 \
    /lib/aarch64-linux-gnu/libz.so.1 \
    /all-files/linux/arm64/lib/aarch64-linux-gnu/

COPY --from=arm64-chef \
    /lib/ld-linux-aarch64.so.1 \
    /all-files/linux/arm64/lib

# AMD64
COPY --from=amd64-chef \
    /lib/x86_64-linux-gnu/libgcc_s.so.1 \
    /lib/x86_64-linux-gnu/libm.so.6 \
    /lib/x86_64-linux-gnu/libc.so.6 \
    /lib/x86_64-linux-gnu/libz.so.1 \
    /all-files/linux/amd64/lib/x86_64-linux-gnu/

COPY --from=amd64-chef \
    /lib64/ld-linux-x86-64.so.2 \
    /all-files/linux/amd64/lib64/

# Common files - certs
COPY --from=amd64-chef \
    /etc/ssl/certs/ca-certificates.crt \
    /all-files/linux/amd64/etc/ssl/certs/
COPY --from=arm64-chef \
    /etc/ssl/certs/ca-certificates.crt \
    /all-files/linux/arm64/etc/ssl/certs/

WORKDIR /app

# Install dependencies for cross-compilation and protobuf
RUN dpkg --add-architecture arm64 \
    && apt-get update \
    && apt-get install -y \
    protobuf-compiler \
    g++-aarch64-linux-gnu \
    libc6-dev-arm64-cross \
    libzip-dev:arm64 \
    ca-certificates \
    && rustup target add aarch64-unknown-linux-gnu \
    && rustup toolchain install stable-aarch64-unknown-linux-gnu --force-non-host \
    && rm -rf /var/lib/apt/lists/*

# Build dependencies - this is the caching Docker layer!
RUN set -eux; \
    feature_flags=""; \
    if [ -n "${CARGO_FEATURES}" ]; then \
        feature_flags="--features ${CARGO_FEATURES}"; \
    fi; \
    case ${TARGETARCH} in \
        arm64) PKG_CONFIG_SYSROOT_DIR=/ CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc cargo chef cook --target=aarch64-unknown-linux-gnu --release --recipe-path recipe.json ${feature_flags} ;; \
        amd64) cargo chef cook --release --recipe-path recipe.json ${feature_flags} ;; \
        *) exit 1 ;; \
    esac

# Copy the source code
COPY . /app

# Build application - this is the caching Docker layer!
RUN set -eux; \
    feature_flags=""; \
    if [ -n "${CARGO_FEATURES}" ]; then \
        feature_flags="--features ${CARGO_FEATURES}"; \
    fi; \
    case ${TARGETARCH} in \
        arm64) PKG_CONFIG_SYSROOT_DIR=/ CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc cargo build --target=aarch64-unknown-linux-gnu --release ${feature_flags} ;; \
        amd64) cargo build --release ${feature_flags} ;; \
        *) exit 1 ;; \
    esac

# Copy all the dependencies to a separate folder
RUN set -ex; \
    # Determine target (source folder for the binary and env files)
    case ${TARGETARCH} in \
    arm64) target='/app/target/aarch64-unknown-linux-gnu/release';; \
    amd64) target='/app/target/release';; \
    *) exit 1 ;; \
    esac; \
    # Copy files from the target folder to app folder
    cp $target/unleash-edge     /all-files/${TARGETPLATFORM}/app

# Always include MIT license in the image (OSS + enterprise)
RUN cp /app/LICENSE /all-files/${TARGETPLATFORM}/LICENSE

# Only include enterprise license for enterprise builds
RUN set -eux; \
    if echo ",${CARGO_FEATURES}," | grep -q ",enterprise,"; then \
        cp /app/LICENSE-ENTERPRISE.md /all-files/${TARGETPLATFORM}/LICENSE-ENTERPRISE.md; \
    fi

## Create a passwd to avoid running as root
FROM --platform=$BUILDPLATFORM ubuntu:25.04 AS passwdsource

RUN useradd -u 10001 edgeuser

# # Create a single layer image
FROM scratch AS runtime

# Make build arguments available in the runtime stage
ARG TARGETPLATFORM
ARG TARGETARCH

WORKDIR /app

# Copy the binary and the environment files from the pre-runtime stage as a single layer
COPY --from=builder /all-files/${TARGETPLATFORM} /
COPY --from=passwdsource /etc/passwd /etc/passwd

USER edgeuser
# Expose the port that the application listens on.
EXPOSE 3063

ENTRYPOINT ["/app/unleash-edge"]
