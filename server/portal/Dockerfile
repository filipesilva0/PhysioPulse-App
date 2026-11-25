FROM rust:1.97-slim-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10002 physiopulse-site
COPY --from=build /app/target/release/physiopulse-site /usr/local/bin/physiopulse-site
USER physiopulse-site
EXPOSE 8090
CMD ["physiopulse-site"]
