// Package webui embeds the built web frontend (web/dist) into the binary.
// The dist directory is populated by `make web` before `go build`.
package webui

import (
	"embed"
	"io/fs"
)

//go:embed all:dist
var distFS embed.FS

// FS returns the embedded dist subtree as an fs.FS rooted at dist/.
func FS() fs.FS {
	sub, err := fs.Sub(distFS, "dist")
	if err != nil {
		panic("webui: dist subtree missing: " + err.Error())
	}
	return sub
}
