# Security Policy

## Supported versions

Miyo is pre-1.0 and released frequently. Please update to the latest release
(`About → Check for Updates`) before reporting an issue — fixes land in the next patch.

## Reporting a vulnerability

**Do not open a public issue for security vulnerabilities.**

Please report privately via one of:

- GitHub Security Advisories for this repository (preferred):
  https://github.com/weihubeats/Miyo/security/advisories/new
- Email: (add maintainer contact here)

Include: affected version, reproduction steps, impact, and any proposed fix if known.

We aim to acknowledge reports within 72 hours and to publish a fix or mitigation
in the next release for confirmed issues.

## Threat model highlights

- Environment variable values are encrypted at rest with AES-256-GCM
  (master key stored outside the database, `0600` permissions).
- The Agent control plane binds loopback (`127.0.0.1`) only and authenticates
  with a per-install token file.
- No telemetry: request/response data never leaves the local machine.
