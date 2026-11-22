# Production Deployment

## Recommended topology

```
internet ──TLS──▶ nginx/caddy (443) ──▶ sshield-server 127.0.0.1:8787
```

Keep the server bound to loopback or a management network; terminate TLS at the proxy.

### systemd unit

```ini
[Unit]
Description=SSHield Enterprise
After=network.target postgresql.service

[Service]
User=sshield
Group=sshield
ExecStart=/opt/sshield/bin/sshield-server -config /etc/sshield/sshield-server.yaml
Environment=SSHIELD_ENV=prod
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/sshield

[Install]
WantedBy=multi-user.target
```

`-config` is optional; without it the server discovers `$SSHIELD_CONFIG`, then
`<data_dir>/sshield-server.yaml`. `./sshield-server -version` prints the build version.

### nginx

```nginx
server {
    listen 443 ssl http2;
    server_name sshield.example.com;

    ssl_certificate     /etc/letsencrypt/live/sshield.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/sshield.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto https;
    }

    # websockets: approvals + terminals
    location /ws/ {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 3600s;
    }
}
```

The SPA is embedded in the binary — no static file serving needed. Terminal websockets can be
long-lived; raise `proxy_read_timeout` as above.

If you enable `metrics.enabled`, the server exposes unauthenticated `GET /metrics` in
Prometheus text format. Either scrape it from localhost or expose it through a proxy
location restricted to your monitoring network, e.g.:

```nginx
location = /metrics {
    proxy_pass http://127.0.0.1:8787;
    allow 10.0.0.0/8;   # monitoring network
    deny all;
}
```

### Cross-origin deployments

If the UI is served from a different origin than the API, set
`server.allowed_origins = ["https://ui.example.com"]`. Same-origin serving needs nothing.

## Docker Compose

A reference `docker-compose.yml` is provided at the repo root: it starts PostgreSQL plus
sshield-server, with a named volume for the data dir. Essentials to get right:

- **The data volume must be persistent.** It holds `server.key` — the single encryption root
  for stored SSH credentials and TOTP secrets. If the volume is recreated, every stored
  credential and enrolled TOTP secret becomes undecryptable. Back the key up separately (see
  [backup.md](backup.md)).
- Point `storage.db_url` at the compose `postgres` service (e.g. via the
  `SSHIELD_STORAGE_DB_URL` environment override).
- Liveness probe: `GET /api/health` (unauthenticated; also returns the build `version`).
- Migrations run automatically at boot; the data volume and the database volume are
  independent — losing the database volume is recoverable from a `pg_dump`, losing the data
  volume without a `server.key` backup is not.

## Version identification

- `./sshield-server -version` prints the build version.
- The version is injected at build time via
  `go build -ldflags "-X github.com/yuantang/sshield/internal/buildinfo.Version=<tag>"`;
  binaries built without it report `dev`.
- `GET /api/health` always reports the running version — useful for rollout verification.

## Hardening checklist

- [ ] TLS enabled (proxy or `server.tls_cert`/`tls_key`) — cookies flip to Secure automatically
- [ ] `bootstrap-admin-password.txt` deleted after first login
- [ ] Admin password rotated; `enforce_totp` considered for admin accounts
- [ ] `/metrics` (if enabled) restricted to the monitoring network
- [ ] Backups of the PostgreSQL database and `server.key` scheduled per [backup.md](backup.md) —
      key stored separately from the DB dump, offline
- [ ] Login lockout thresholds reviewed (`security.*`)
- [ ] Retention windows reviewed (`retention.*`) if recordings must not grow indefinitely
- [ ] MCP tokens audited (Settings → API Tokens, check `last_used`)
- [ ] Audit chain verified periodically (Audit → Verify chain)

## Upgrades

Single static binary: build the new release, replace it, restart. Schema migrations and the
bootstrap sequence are idempotent — existing users, grants and the audit chain are preserved.
