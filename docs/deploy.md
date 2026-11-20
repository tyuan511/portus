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
After=network.target

[Service]
User=sshield
Group=sshield
ExecStart=/opt/sshield/bin/sshield-server
Environment=SSHIELD_CONFIG=/etc/sshield/sshield-server.toml
Environment=SSHIELD_ENV=prod
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/sshield

[Install]
WantedBy=multi-user.target
```

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

### Cross-origin deployments

If the UI is served from a different origin than the API, set
`server.allowed_origins = ["https://ui.example.com"]`. Same-origin serving needs nothing.

## Hardening checklist

- [ ] TLS enabled (proxy or `server.tls_cert`/`tls_key`) — cookies flip to Secure automatically
- [ ] `bootstrap-admin-password.txt` deleted after first login
- [ ] Admin password rotated; `enforce_totp` considered for admin accounts
- [ ] `vault.unlock_passphrase_file` unset unless the host is single-purpose; vault stays locked when idle
- [ ] Backups of `sshield.db` + `vault.enc` + `server.key` stored together, offline
- [ ] Login lockout thresholds reviewed (`security.*`)
- [ ] MCP tokens audited (Settings → API Tokens, check `last_used`)
- [ ] Audit chain verified periodically (Audit → Verify chain)

## Upgrades

Single static binary: build the new release, replace it, restart. Schema migrations and the
bootstrap sequence are idempotent — existing users, grants and the audit chain are preserved.
