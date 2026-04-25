FROM rust:latest AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    fontconfig \
    libfreetype6 \
    fonts-dejavu-core \
 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/clockbot /usr/local/bin/clockbot
CMD ["clockbot"]
