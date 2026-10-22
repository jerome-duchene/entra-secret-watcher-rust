FROM rust:alpine AS build
WORKDIR /src

RUN apk add --no-cache musl-dev build-base

# Build the dependencies first so they are cached until Cargo.toml / Cargo.lock change
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src

# Version of the release (the git tag, with or without its "v"), so that the binary reports it in
# its logs. Without it, the version of Cargo.toml is kept.
ARG VERSION=
RUN if [ -n "$VERSION" ]; then \
        VERSION="${VERSION#v}"; \
        echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$' \
            || { echo "Invalid VERSION '$VERSION'"; exit 1; }; \
        sed -i "1,/^version = /s/^version = .*/version = \"$VERSION\"/" Cargo.toml; \
        grep -q "^version = \"$VERSION\"" Cargo.toml || { echo "Could not set the version"; exit 1; }; \
    fi

# Make sure cargo sees the real sources as newer than the placeholders
RUN touch src/main.rs src/lib.rs && cargo build --release

# ---

FROM alpine:3 AS runtime
WORKDIR /app

RUN apk add --no-cache ca-certificates \
    && addgroup -S appgroup && adduser -S appuser -G appgroup
USER appuser

COPY --from=build /src/target/release/entra-secret-watcher .

ENV Server__Listen=0.0.0.0:8080
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/health || exit 1

ENTRYPOINT ["/app/entra-secret-watcher"]
