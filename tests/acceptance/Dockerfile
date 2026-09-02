ARG BASE_IMAGE=debian:12-slim
FROM rust:1.85-bookworm AS build
WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM ${BASE_IMAGE}
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl openssl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /workspace/target/release/sbctl /usr/local/bin/sbctl
COPY tests/acceptance/verify.sh /usr/local/lib/sbctl-acceptance/verify.sh
RUN chmod 0755 /usr/local/bin/sbctl /usr/local/lib/sbctl-acceptance/verify.sh
ENTRYPOINT ["/usr/local/lib/sbctl-acceptance/verify.sh"]
