.PHONY: build web dev start deps-up deps-down test vet fmt clean cover

# Version injected into internal/buildinfo.Version at build time
# (reported by `bulwark-server -version` and GET /api/health).
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)
DEV_DB_URL ?= postgres://bulwark:bulwark@127.0.0.1:15432/bulwark?sslmode=disable

# Build the web frontend into internal/webui/dist, then the single binary.
build: web
	go build -o bin/bulwark-server \
		-ldflags "-s -w -X github.com/tyuan511/bulwark/internal/buildinfo.Version=$(VERSION)" \
		./cmd/bulwark-server

web:
	cd web && pnpm install --frozen-lockfile 2>/dev/null || pnpm install
	cd web && pnpm build
	rm -rf internal/webui/dist
	cp -R web/dist internal/webui/dist

# Start prerequisite dev services (PostgreSQL on 127.0.0.1:15432) and wait until healthy.
deps-up:
	docker compose -f docker-compose.dev.yml up -d --wait

# Stop prerequisite dev services.
deps-down:
	docker compose -f docker-compose.dev.yml down

# One-click development startup: prerequisite Postgres (15432) + Go server (8787) + Vite (5173).
dev: deps-up
	@trap 'kill 0' INT TERM EXIT; \
	BULWARK_STORAGE_DB_URL="$(DEV_DB_URL)" go run ./cmd/bulwark-server & \
	(cd web && pnpm dev) & \
	wait

start: dev

test:
	go test ./...

vet:
	go vet ./...

# Run tests with coverage; prints the total statement coverage as the last line.
cover:
	go test ./... -coverprofile=coverage.out && go tool cover -func=coverage.out | tail -1

fmt:
	gofmt -w .

clean:
	rm -rf bin internal/webui/dist
