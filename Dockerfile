FROM rust:bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=builder /app/target/release/image-date-tagger /app/
COPY templates/ ./templates/
COPY static/ ./static/
COPY data/ ./data/

EXPOSE 8000

CMD ["./image-date-tagger"]
