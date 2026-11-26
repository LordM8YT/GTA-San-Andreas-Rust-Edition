# The SARE directory (docs/sare-directory.md) as a small container.
# Build context: native/.  docker build -f crates/net/directory.Dockerfile .
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p sa-net --bin sa-directory

FROM debian:bookworm-slim
COPY --from=build /src/target/release/sa-directory /usr/local/bin/sa-directory
USER nobody
# Listens on 0.0.0.0:$PORT, or 0.0.0.0:7780 when PORT is not set.
ENV PORT=7780
EXPOSE 7780
CMD ["sa-directory"]
