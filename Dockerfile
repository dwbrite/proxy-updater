FROM rust:1.70 as builder
COPY . /builder
WORKDIR /builder
RUN cargo build --release

FROM debian
COPY --from=builder /builder/target/release/proxy-updater /proxy-updater
CMD ["/proxy-updater"]
