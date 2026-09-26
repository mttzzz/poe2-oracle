# oracle.pushka.biz: the oracle-web server with the site, the player guide and the guide's pictures.
# .github/workflows/deploy.yml builds it through the shared deploy workflow, and the pod runs it
# with deploy/values.yaml. The lane's dev server (lanes/dev.sh) runs the same server from the
# checkout.

# The lane runner's base (lanes/runner.Dockerfile) at the Rust release the lane runs.
ARG RUST_IMAGE=rust:1.98-bookworm

# The server's own workspace: the root manifest, Cargo.lock and the two crates' manifests, with
# stub sources, trimmed by deploy/trim-workspace.sh (which says why). The output changes only when
# the server's dependencies do, so a Cargo.lock change that concerns only the app leaves the
# builder's dependency layer cached.
FROM ${RUST_IMAGE} AS plan
WORKDIR /plan
COPY Cargo.toml Cargo.lock ./
COPY crates/oracle-protocol/Cargo.toml crates/oracle-protocol/
COPY crates/oracle-web/Cargo.toml crates/oracle-web/
COPY deploy/trim-workspace.sh /usr/local/bin/
RUN mkdir crates/oracle-protocol/src crates/oracle-web/src \
 && touch crates/oracle-protocol/src/lib.rs \
 && echo 'fn main() {}' > crates/oracle-web/src/main.rs \
 && trim-workspace.sh

FROM ${RUST_IMAGE} AS builder
WORKDIR /build
COPY --from=plan /plan/ ./
# The dependencies, compiled against the stubs.
RUN cargo build --release --locked -p oracle-web
COPY crates/oracle-protocol crates/oracle-protocol
COPY crates/oracle-web crates/oracle-web
# Cargo goes by modification times, and the stubs were compiled after the checkout wrote the real
# sources: touched, the real sources are newer and get compiled.
RUN find crates -name '*.rs' -exec touch {} + \
 && cargo build --release --locked -p oracle-web

# The guide, a book per language that docs/guide/build.sh builds, with a pinned mdBook checked
# against its release's sha256. The builder's image: already pulled, and it has curl. lanes/dev.sh
# reads both pins from here.
FROM ${RUST_IMAGE} AS guide
ARG MDBOOK_VERSION=v0.5.4
ARG MDBOOK_SHA256=3f28de05dafca9d0f2eab99c662116b0e37b89b1d96a08f8f430b9eeae958cd7
RUN archive="mdbook-${MDBOOK_VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
 && curl -sSfL -o "/tmp/$archive" \
      "https://github.com/rust-lang/mdBook/releases/download/${MDBOOK_VERSION}/$archive" \
 && echo "${MDBOOK_SHA256}  /tmp/$archive" | sha256sum -c - \
 && tar -xzf "/tmp/$archive" -C /usr/local/bin \
 && rm "/tmp/$archive"
COPY docs/guide /docs/guide
RUN /docs/guide/build.sh /guide

# glibc and libgcc for the binary, no shell, and a non-root user (65532). The server's TLS is rustls
# with built-in root certificates: the image's CA bundle goes unused.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /build/target/release/oracle-web /app/oracle-web
COPY --from=guide /guide /app/guide
COPY site /app/site
COPY docs/guide/src/images /app/images
EXPOSE 8080
ENTRYPOINT ["/app/oracle-web"]
