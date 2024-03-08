# Use rust base image
FROM rust:1.70 as builder

WORKDIR /builder

# Copy the Cargo.toml and Cargo.lock files separately
COPY ./Cargo.toml ./Cargo.lock ./

# Create a dummy source file to compile dependencies
RUN mkdir src/ && \
    echo "fn main() {println!(\"if you see this, the cache was not used\")}" > src/main.rs && \
    cargo build --release
RUN rm -f target/release/deps/proxy_updater*

COPY . .

RUN cargo build --release

# Use debian as the final base image
FROM debian

# Install dependencies
RUN apt-get update && \
    apt-get install -y libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Copy the built binary from the builder stage
COPY --from=builder /builder/target/release/proxy-updater /proxy-updater
COPY --from=builder /builder/src/nginx.conf.tpl /src/nginx.conf.tpl

CMD ["/proxy-updater"]
