// Package buildinfo carries the build version injected via
// -ldflags "-X github.com/yuantang/sshield/internal/buildinfo.Version=...".
package buildinfo

// Version is overridden at build time; "dev" otherwise.
var Version = "dev"
