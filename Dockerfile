FROM rustlang/rust:nightly as builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    musl-tools ca-certificates && \
    rustup target add x86_64-unknown-linux-musl

COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM scratch

COPY --from=builder target/x86_64-unknown-linux-musl/release/postgres-with-quic /postgres-with-quic


ENTRYPOINT ["/postgres-with-quic"]
