# Dockerfile multi-stage pour Qonso (Rust MOLAP Server + React Frontend)

# 1. Build du frontend React
FROM node:20-alpine AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package*.json ./
RUN npm install
COPY frontend/ ./
RUN npm run build

# 2. Build du backend Rust
FROM rust:1.80-bookworm AS backend-builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
RUN cargo build --release

# 3. Image finale d'exécution légère
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Récupération du binaire Rust et des assets statiques frontend
COPY --from=backend-builder /app/target/release/qonso /app/qonso
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Création du répertoire de données persistant (WAL et snapshots)
RUN mkdir -p /app/data

ENV RUST_LOG=info
EXPOSE 3001

CMD ["/app/qonso"]
