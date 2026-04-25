# ---------------------------------------------------
# Stage 1: Builder (Heavy - Contains Source & Compiler)
# ---------------------------------------------------
FROM rust:1.80-slim-bookworm AS builder

# Install necessary C-dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# --- CACHE LAYER OPTIMIZATION ---
# 1. Copy only dependency manifests
COPY Cargo.toml Cargo.lock ./
# 2. Create dummy source files to trigger dependency compilation
RUN mkdir src && echo "fn main() { println!(\"if you see this, the build broke\") }" > src/main.rs && touch src/lib.rs
# 3. Build only the dependencies (This layer gets cached!)
RUN cargo build --release --all-features

# --- ACTUAL CODE COMPILATION ---
# 4. Remove dummy files and copy the real project
RUN rm -rf src
COPY . .

# Force SQLx offline mode
ENV SQLX_OFFLINE=true

# 5. Update timestamp of main to force rebuild of OUR code, not dependencies
RUN touch src/main.rs
RUN cargo build --release --all-features

# ---------------------------------------------------
# Stage 2: Runtime (Lightweight & Secure - Binary Only)
# ---------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install CA certificates for HTTPS/API calls
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy ONLY the compiled binary
COPY --from=builder /app/target/release/kaspa-pulse /usr/local/bin/kaspa-pulse

# Ensure the binary is executable
RUN chmod +x /usr/local/bin/kaspa-pulse

# Define the entrypoint
ENTRYPOINT ["kaspa-pulse"]
