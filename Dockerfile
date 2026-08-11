FROM rust:1.97.1-bookworm AS builder

WORKDIR /src

# Keep the image build independent of working/Paper and other local-only data.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY tools ./tools
COPY fuzz ./fuzz

RUN cargo build --locked --release -p rivet-server

FROM debian:bookworm-slim

RUN useradd --create-home --uid 10001 rivet
COPY --from=builder /src/target/release/rivet-server /usr/local/bin/rivet-server

USER rivet
WORKDIR /home/rivet
EXPOSE 25565
ENTRYPOINT ["/usr/local/bin/rivet-server"]
CMD ["--host", "0.0.0.0", "--port", "25565"]
