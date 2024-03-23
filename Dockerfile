# Use the nightly version of Rust
FROM rust:latest

# Set the working directory
WORKDIR /usr/src/webrtc-streaming

# Copy the Cargo.toml and Cargo.lock files
COPY Cargo.toml ./

# Copy the source code
COPY src ./src


# Build the dependencies
RUN apt-get update && \
    apt-get install -y curl build-essential libpq-dev openssl libssl-dev pkg-config lld clang libclang-dev ca-certificates && \
    update-ca-certificates

# Update rust
RUN rustup update

RUN cargo build --release

FROM ubuntu:jammy-20240212

# Install the dependencies
RUN apt-get update && \
    apt-get install -y openssl libssl-dev
# Set the working directory
WORKDIR /usr/src/webrtc-streaming

# Copy the binary from the build image
COPY --from=0 /usr/src/webrtc-streaming/target/release/webrtc-streaming .
COPY --from=0 /usr/src/webrtc-streaming/templates ./templates
COPY --from=0 /usr/src/webrtc-streaming/src/ ./src


# Expose the port
EXPOSE 80

# Run the binary
CMD ["./webrtc-streaming"]