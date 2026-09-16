# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS builder

WORKDIR /usr/src/traceroute
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libcap2-bin && rm -rf /var/lib/apt/lists/*
WORKDIR /opt/traceroute
COPY --from=builder /usr/src/traceroute/target/release/traceroute /opt/traceroute/traceroute
RUN setcap cap_net_raw+ep /opt/traceroute/traceroute

ENTRYPOINT ["/opt/traceroute/traceroute"]