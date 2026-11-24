// Package buildinfo carries the build version injected via
// -ldflags "-X github.com/tyuan511/bulwark/internal/buildinfo.Version=...".
package buildinfo

// Version is overridden at build time; "dev" otherwise.
var Version = "dev"
