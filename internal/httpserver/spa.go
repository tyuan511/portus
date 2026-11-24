package httpserver

import (
	"io/fs"
	"path"
	"strings"

	"github.com/gofiber/fiber/v2"

	"github.com/tyuan511/bulwark/internal/webui"
)

// mountSPA serves the embedded single-page app for every non-API path,
// falling back to index.html so client-side routing deep links work.
func (s *Server) mountSPA(app *fiber.App) {
	dist := webui.FS()

	app.Use(func(c *fiber.Ctx) error {
		p := c.Path()
		if strings.HasPrefix(p, "/api") || strings.HasPrefix(p, "/ws") || strings.HasPrefix(p, "/mcp") {
			return c.Next()
		}
		clean := strings.TrimPrefix(path.Clean(p), "/")
		if clean == "" {
			clean = "index.html"
		}
		if data, err := fs.ReadFile(dist, clean); err == nil {
			c.Set("Content-Type", contentTypeFor(clean))
			return c.Send(data)
		}
		// SPA fallback: unknown routes render the client entrypoint.
		if data, err := fs.ReadFile(dist, "index.html"); err == nil {
			c.Set("Content-Type", "text/html; charset=utf-8")
			return c.Send(data)
		}
		return c.Next()
	})
}

func contentTypeFor(name string) string {
	switch {
	case strings.HasSuffix(name, ".html"):
		return "text/html; charset=utf-8"
	case strings.HasSuffix(name, ".js"), strings.HasSuffix(name, ".mjs"):
		return "text/javascript; charset=utf-8"
	case strings.HasSuffix(name, ".css"):
		return "text/css; charset=utf-8"
	case strings.HasSuffix(name, ".json"):
		return "application/json"
	case strings.HasSuffix(name, ".svg"):
		return "image/svg+xml"
	case strings.HasSuffix(name, ".png"):
		return "image/png"
	case strings.HasSuffix(name, ".ico"):
		return "image/x-icon"
	case strings.HasSuffix(name, ".woff2"):
		return "font/woff2"
	default:
		return "application/octet-stream"
	}
}
