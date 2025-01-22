# Start with the official Rust image
FROM rust:1.72 as builder

# Set the working directory
WORKDIR /usr/src/solana-mcp-server

# Copy the manifest and build dependencies first for caching
COPY Cargo.toml .

# Create a blank main file to trick Cargo into fetching dependencies
RUN echo "fn main() {}" > src/main.rs
RUN mkdir src
RUN cargo build --release || true

# Now copy the entire source code
COPY . .

# Build the actual executable
RUN rm -f target/release/deps/solana_mcp_server*
RUN cargo build --release

# Now, create a minimal image for running the application
FROM debian:buster-slim

# Set the working directory
WORKDIR /usr/src/app

# Copy the built binary from the builder stage
COPY --from=builder /usr/src/solana-mcp-server/target/release/solana-mcp-server .

# Set the environment variable for the Solana RPC URL
ENV SOLANA_RPC_URL=https://api.mainnet-beta.solana.com

# Run the binary
CMD ["./solana-mcp-server"]