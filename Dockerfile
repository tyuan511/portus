# Multi-stage build: web (pnpm) -> server (go, embedded dist) -> distroless runtime.
#
# Build:  docker build -t sshield-server .
# Run:    docker run -p 8787:8787 -v ./data:/data sshield-server
#         (see docker-compose.yml for the full stack with PostgreSQL)
#
# Override the build version with:  docker build --build-arg VERSION=v1.2.3 .

# ---------------------------------------------------------------- stage 1: web
FROM node:22-alpine AS web

RUN corepack enable && corepack prepare pnpm@11.24.0 --activate

WORKDIR /web

# pnpm-lock.yaml exists in the repo, so --frozen-lockfile is used for
# reproducible installs. Fallback: if the lockfile is ever absent, replace
# "pnpm install --frozen-lockfile" below with a plain "pnpm install".
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile

COPY web/ ./
# "pnpm build" runs "tsc -b && vite build" and emits web/dist.
RUN pnpm build

# ------------------------------------------------------------- stage 2: server
FROM golang:1.26-alpine AS build

# Version is baked into internal/buildinfo.Version (also reported by
# GET /api/health and `sshield-server -version`).
ARG VERSION=docker

WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY cmd/ cmd/
COPY internal/ internal/

# Emulate `make web`: place the built frontend where //go:embed expects it.
COPY --from=web /web/dist internal/webui/dist

RUN CGO_ENABLED=0 GOOS=linux go build -trimpath \
    -ldflags "-s -w -X github.com/yuantang/sshield/internal/buildinfo.Version=${VERSION}" \
    -o /out/sshield-server ./cmd/sshield-server

# ------------------------------------------------------------- stage 3: runtime
FROM gcr.io/distroless/static-debian12

# No HEALTHCHECK here on purpose: distroless/static has no shell, no wget/curl,
# so a liveness probe cannot run inside the container. Check GET /api/health
# (unauthenticated, reports the build version) from outside the container
# instead, or swap this stage for alpine if an in-container check is required.
#
# The binary is fully static (CGO_ENABLED=0); distroless/static ships the CA
# certificates and tzdata needed for outbound webhook HTTPS calls.

COPY --from=build /out/sshield-server /sshield-server

EXPOSE 8787
VOLUME /data

# Entrypoint is the bare binary (no shell in distroless). Extra CLI flags can
# be appended at run time, e.g. `docker run ... sshield-server -config /data/sshield-server.yaml`.
# In practice no flag is needed: config discovery falls back to
# <data_dir>/sshield-server.yaml (i.e. /data/sshield-server.yaml) and every
# setting can be overridden via SSHIELD_* environment variables (see
# docker-compose.yml).
ENTRYPOINT ["/sshield-server"]
