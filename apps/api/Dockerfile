FROM rust:1.95.0-slim-trixie AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY apps/api apps/api
COPY apps/worker apps/worker
# Cargo treats a crate as fresh when its sources are older than the cached
# build, and COPY keeps the timestamps from the build context. A changed file
# with an older timestamp, such as one from another checkout, would ship the
# cached binary. Cleaning the workspace's own crates rebuilds them every time;
# their dependencies stay cached.
# Without --locked, the clean would rewrite a Cargo.lock that no longer
# matches the manifests, and the locked build would then accept it.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo clean --release --locked --workspace \
    && cargo build --release --locked --bin api \
    && cp target/release/api /usr/local/bin/api

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home api
COPY --from=build /usr/local/bin/api /usr/local/bin/api
USER api
EXPOSE 3002
CMD ["api", "serve"]
