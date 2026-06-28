FROM rust:bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>/dev/null || true

COPY src/ ./src/
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app

COPY --from=builder /app/target/release/image-date-tagger /app/
COPY templates/ ./templates/
COPY static/ ./static/
COPY data/ ./data/

EXPOSE 8000

CMD ["./image-date-tagger"]
