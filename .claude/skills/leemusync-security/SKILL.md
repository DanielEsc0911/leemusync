---
name: leemusync-security
description: Use when a LeemuSync change touches file paths or writes, crypto, keys or credentials, network or IPC, parsing of untrusted data (manifests, profiles, config, deep links, IPC frames), launching processes, dependencies, CI or release signing, and when reviewing such a change.
---

# Security review (B2)

Threat model and controls: `docs/specs/security.md`, which implements rule B2 from `docs/rules.md`. Run this checklist on every matching change and paste the result into the PR.

## Checklist
- [ ] **Untrusted input** (remote objects, manifests, profiles, config, deep links, IPC): validated in `core`/`api` before use; size limits; fuzz target added or extended
- [ ] **Paths**: go through the `core` validator; inside a grant; no symlink following on write; Windows reserved names, ADS and UNC paths rejected
- [ ] **Writes**: atomic, with a backup first (reliability I1, I4)
- [ ] **Crypto**: only `crypto` crate primitives; AAD binds kind + id; random nonces; secrets zeroized; no new algorithms
- [ ] **Credentials**: OS keystore; never in config, logs, IPC events or error messages
- [ ] **Network**: outbound only; rustls; certificate checks never disabled; no new listener, ever
- [ ] **IPC**: same-user peer check intact; new request types validated
- [ ] **Processes**: user-chosen executable + argv array; no shell
- [ ] **Deep links**: navigate only; actions need confirmation
- [ ] **Dependencies**: justified in the PR (P1); `cargo deny check` green; license on the allow list
- [ ] **unsafe**: none outside `crates/bridge`
- [ ] **CI/release**: actions pinned by SHA; least-privilege permissions; signing untouched or reviewed
- [ ] **Privacy**: no telemetry; logs redacted

## Red flags: stop and redesign
`sh -c` / `cmd /c` · `std::fs::write` straight onto a target path · `danger_accept_invalid_certs` · `unwrap()` on remote data · a profile field that names a binary · downloading anything executable or any "update" · `TcpListener` · printing a token "for debugging".

## Reporting
Vulnerabilities are never discussed in public issues or PRs. See `SECURITY.md`.
