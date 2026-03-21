# --- Build Stage ---
FROM rust:1.90-slim-bookworm AS builder

WORKDIR /app
COPY . .

# Install dependencies for compilation
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Build the project
RUN cargo build --release

# --- Final Stage ---
FROM debian:bookworm-slim

# Install Google Chrome and dependencies
RUN apt-get update && apt-get install -y \
    curl \
    gnupg \
    ca-certificates \
    xz-utils \
    procps \
    && curl -fSsL https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb -o /tmp/chrome.deb \
    && apt-get install -y /tmp/chrome.deb \
    && rm /tmp/chrome.deb \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /app/target/release/chrome-debug-mcp /usr/local/bin/chrome-debug-mcp

# Expose remote debugging port (though typically used via stdio in MCP)
EXPOSE 9222

# Default entrypoint
ENTRYPOINT ["chrome-debug-mcp"]

# Default arguments (headless is usually required in Docker)
CMD ["--headless"]
