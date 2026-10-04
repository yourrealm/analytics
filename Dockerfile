# analytics image: one binary (ingest, the owner's JSON API and the built
# React dashboard) and DB-IP's country database on distroless.
#
# Build context is the repo root; .dockerignore narrows it to server/, web/,
# the package manifests and the GeoIP script. realm.tsx describes the deployment and never runs in the
# container.
#
# $BUILDPLATFORM + cross-compilation per $TARGETARCH: nothing here executes
# target code, so both arches build natively on one runner instead of one of
# them running under QEMU (the Realm updater's recipe). Unlike the updater,
# rusqlite's bundled sqlite3.c needs a C cross-compiler, hence Debian's
# gcc-*-linux-gnu packages, glibc targets, and a glibc runtime (distroless/cc).

# --- server: cross-compiled binary --------------------------------------------
FROM --platform=$BUILDPLATFORM rust:1.96-bookworm AS server
ARG TARGETARCH
# Pick the target, and a cross gcc only when the target is not the host arch
# (Debian ships no gcc-<host>-linux-gnu package). Cargo's linker and cc-rs's
# compiler are set through /cross.env so the build step below stays cacheable
# on its own.
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) triple=x86_64-linux-gnu; target=x86_64-unknown-linux-gnu ;; \
      arm64) triple=aarch64-linux-gnu; target=aarch64-unknown-linux-gnu ;; \
      *) echo "TARGETARCH is '$TARGETARCH' - build with buildx/BuildKit"; exit 1 ;; \
    esac; \
    rustup target add "$target"; \
    printf 'export RUST_TARGET=%s\n' "$target" > /cross.env; \
    if [ "$(dpkg --print-architecture)" != "$TARGETARCH" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends \
        "gcc-$(printf %s "$triple" | tr _ -)" "libc6-dev-$TARGETARCH-cross"; \
      rm -rf /var/lib/apt/lists/*; \
      upper="$(printf %s "$target" | tr a-z- A-Z_)"; \
      lower="$(printf %s "$target" | tr - _)"; \
      printf 'export CARGO_TARGET_%s_LINKER=%s-gcc\nexport CC_%s=%s-gcc\n' \
        "$upper" "$triple" "$lower" "$triple" >> /cross.env; \
    fi
WORKDIR /app
COPY server/ ./server/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/server/target \
    set -eux; . /cross.env; \
    cd server; \
    cargo build --release --locked --target "$RUST_TARGET"; \
    cp "target/$RUST_TARGET/release/analytics" /analytics

# --- web: the React dashboard --------------------------------------------------
# Static files, so it builds once on the build platform for both arches.
FROM --platform=$BUILDPLATFORM node:24-bookworm-slim AS web
RUN corepack enable
WORKDIR /app
COPY package.json pnpm-lock.yaml .npmrc ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile
COPY web/ ./web/
RUN pnpm build:web

# --- geo: DB-IP Lite country database (CC BY 4.0) -------------------------------
# Arch-neutral data, so it runs on the build platform. Monthly data: rebuild the
# image to refresh it.
FROM --platform=$BUILDPLATFORM rust:1.96-bookworm AS geo
COPY scripts/geoip.sh /geoip.sh
RUN sh /geoip.sh /geo.mmdb

# --- runtime: the binary, the dashboard, the database and glibc ----------------
# No shell: the compose healthcheck is exec-form (`/app/analytics healthcheck`).
FROM gcr.io/distroless/cc-debian12
COPY --from=server /analytics /app/analytics
COPY --from=geo /geo.mmdb /app/geo.mmdb
COPY --from=web /app/web/dist /app/web
ENV PORT=3000
EXPOSE 3000
ENTRYPOINT ["/app/analytics"]
