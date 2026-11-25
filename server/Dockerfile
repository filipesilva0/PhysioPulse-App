FROM rust:1.97-slim-bookworm AS build
WORKDIR /app
# Cargo.lock pins the crate versions that are known to build on Rust 1.97 (lopdf 0.42 needs let-chains, Rust 1.88+, and declares no rust-version). Keep the -bookworm variant: it must match the runtime image below (glibc).
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# Migrations are embedded at compile time (include_str! in src/db.rs).
COPY migrations ./migrations
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 physiopulse
COPY --from=build /app/target/release/physiopulse-server /usr/local/bin/physiopulse-server
USER physiopulse
EXPOSE 8080
CMD ["physiopulse-server"]
