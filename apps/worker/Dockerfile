FROM rust:1.95.0-slim-trixie AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY apps/api apps/api
COPY apps/worker apps/worker
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --locked --bin worker \
    && cp target/release/worker /usr/local/bin/worker

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home worker
COPY --from=build /usr/local/bin/worker /usr/local/bin/worker
USER worker
EXPOSE 9080
CMD ["worker", "serve"]
