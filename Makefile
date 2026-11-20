.PHONY: build web dev test vet fmt clean

# Build the web frontend into internal/webui/dist, then the single binary.
build: web
	go build -o bin/sshield-server ./cmd/sshield-server

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

fmt:
	gofmt -w .

clean:
	rm -rf bin internal/webui/dist
