.PHONY: build web dev test vet fmt clean cover

# Version injected into internal/buildinfo.Version at build time
# (reported by `sshield-server -version` and GET /api/health).
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)

# Build the web frontend into internal/webui/dist, then the single binary.
build: web
	go build -o bin/sshield-server \
		-ldflags "-s -w -X github.com/yuantang/sshield/internal/buildinfo.Version=$(VERSION)" \
		./cmd/sshield-server

web:
	cd web && pnpm install --frozen-lockfile 2>/dev/null || pnpm install
	cd web && pnpm build
	rm -rf internal/webui/dist
	cp -R web/dist internal/webui/dist

# Development: go server on 8787 + vite dev server (proxies /api,/ws,/mcp).
dev:
	go run ./cmd/sshield-server & cd web && pnpm dev

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
