# syntax=docker/dockerfile:1

# ---- build stage ----
FROM rust:1.98-slim-bookworm AS build
WORKDIR /build
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release && cp target/release/meshcheck /usr/local/bin/meshcheck

# ---- minimal runtime stage ----
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home meshcheck
COPY --from=build /usr/local/bin/meshcheck /usr/local/bin/meshcheck
# Demo meshes are baked into the image; /work may be bind-mounted to audit
# documents from the host.
COPY examples /examples
USER meshcheck
WORKDIR /work
ENTRYPOINT ["meshcheck"]
# Default invocation demos the closed genus-1 torus fixture.
CMD ["/examples/torus9.mesh"]
