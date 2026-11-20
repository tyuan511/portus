package security

import (
	"strings"
	"testing"
)

func TestMasksSecretsInOutput(t *testing.T) {
	raw := "DB_PASSWORD=supersecret123\n" +
		"AWS_KEY=AKIAIOSFODNN7EXAMPLE\n" +
		"token: sk-proj-abcdefghij1234567890\n" +
		"DATABASE_URL=postgres://admin:s3cr3t@10.0.0.5:5432/prod\n" +
		"Authorization: bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9xxx\n"
	masked := MaskSensitiveOutput(raw)

	if strings.Contains(masked, "supersecret123") {
		t.Fatal("password not masked")
	}
	if strings.Contains(masked, "AKIAIOSFODNN7EXAMPLE") {
		t.Fatal("aws key not masked")
	}
	if strings.Contains(masked, "sk-proj-abcdefghij1234567890") {
		t.Fatal("api key not masked")
	}
	if strings.Contains(masked, "s3cr3t") {
		t.Fatal("connection string password not masked")
	}
	if strings.Contains(masked, "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9xxx") {
		t.Fatal("bearer token not masked")
	}
	for _, want := range []string{"[REDACTED]", "[REDACTED_AWS_ACCESS_KEY]", "[REDACTED_API_KEY]", "[REDACTED_TOKEN]"} {
		if !strings.Contains(masked, want) {
			t.Fatalf("missing %s in: %s", want, masked)
		}
	}
}

func TestMasksPEMKey(t *testing.T) {
	raw := "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\nfoo\n-----END RSA PRIVATE KEY-----\ndone"
	masked := MaskSensitiveOutput(raw)
	if strings.Contains(masked, "MIIEowIBAAKCAQEA") {
		t.Fatal("PEM body not masked")
	}
	if !strings.Contains(masked, "[REDACTED_PRIVATE_KEY]") {
		t.Fatal("PEM marker missing")
	}
}

func TestMaskPreservesNormalOutput(t *testing.T) {
	raw := "total 12\ndrwxr-xr-x 2 root root 4096 May 1 12:00 etc\n"
	if got := MaskSensitiveOutput(raw); got != raw {
		t.Fatalf("normal output must be untouched, got %q", got)
	}
}
