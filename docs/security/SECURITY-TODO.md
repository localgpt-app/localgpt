# Security Enhancements TODO

Security considerations for enterprise deployment of LocalGPT.

> Consolidated from the former `SECURITY-COMPARISON.md` (archived). Updated 2026-03-25.

## Progress Summary (vs OpenClaw)

| Category | LocalGPT | OpenClaw | Status |
|----------|----------|----------|--------|
| Prompt Injection | 12 markers, 10 patterns | 10+ patterns | **PARITY** |
| Content Delimiters | 3 types | Similar | **PARITY** |
| Shell Sandbox | Landlock + seccomp (Linux), Seatbelt (macOS) | Docker-ready | **AHEAD** |
| Encryption at Rest | XChaCha20-Poly1305 + Argon2id | Mentioned in TODO | **AHEAD** |
| Rate Limiting | Per-IP rate limiting | Message throttler | **PARITY** |
| Input Validation | JSON only | Zod schemas | **BEHIND** |
| Tool Policies | require_approval only | Full allowlist/denylist | **BEHIND** |
| SSRF Protection | Full (private IP, DNS pinning, redirect validation) | Full (DNS pinning, redirect validation) | **PARITY** |
| Authentication | API key/token auth | Device identity, OAuth, webhook sigs | **PARTIAL** |
| Audit System | Policy signing + audit chain | 50+ security checks | **PARTIAL** |
| Secret Detection | Env var expansion | detect-secrets CI | **BEHIND** |

**Legend**: AHEAD / PARITY / PARTIAL / BEHIND

## Critical (Must Have)

### 1. Authentication & Authorization
- [x] Add API key/token authentication to all HTTP endpoints — `15-gateway-auth` ✅
- [ ] Implement role-based access control (RBAC)
- [ ] Session validation with cryptographic signing
- [ ] SSO integration (SAML/OIDC) for enterprise identity

### 2. Command Execution Sandboxing
- [x] Kernel-level sandbox: Landlock (Linux) and Seatbelt (macOS) — `localgpt-sandbox` crate ✅
- [ ] Docker/container sandbox for cross-platform isolation — `15-docker-sandbox` ❌ (spec in `todo/TODO-15-docker-sandbox.md`)
- [ ] Options for additional hardening:
  - Allowlist of permitted commands
  - seccomp or AppArmor profiles

### 3. Secrets Management
- [x] Encrypt credentials and sessions at rest — XChaCha20-Poly1305 with Argon2id key derivation, CLI commands `encrypt enable/disable/status/rotate` — `15-encrypt-at-rest` ✅
- [ ] Integrate with enterprise secret stores (HashiCorp Vault, AWS Secrets Manager, etc.)

### 4. Network Security
- [x] Binds to localhost only by default ✅
- [x] Per-IP rate limiting — `2-rate-limiting` ✅
- [x] Oversized payload guard — `2-oversized-payload` ✅
- [ ] For multi-user: add TLS termination — `2-tls-auto` ❌
- [ ] Remove permissive CORS (`allow_origin(Any)`)
- [x] **SSRF protection** for web_fetch tool — `ssrf.rs` module ✅
  - [x] Block private IP ranges (10.x, 172.16-31.x, 192.168.x, 127.x, 0.x, CGNAT, multicast, reserved)
  - [x] Block metadata endpoints (169.254.169.254, metadata.google.internal)
  - [x] DNS pinning (resolve and validate before fetch)
  - [x] Redirect validation (each hop re-validated via `ssrf::validate_url`)
  - [x] IPv4-mapped IPv6 (::ffff:x.x.x.x) checked against IPv4 rules
  - [x] Scheme restriction (http/https only)

## High Priority

### 5. Prompt Injection Defenses

**Status**: At parity with OpenClaw. See [SECURITY-COMPARISON.md](./SECURITY-COMPARISON.md) for details.

**Implemented** (in `localgpt/src/agent/sanitize.rs`):
- [x] Marker stripping (12+ LLM injection formats) ✅
- [x] Content delimiters (`<tool_output>`, `<memory_context>`, `<external_content>`) ✅
- [x] Suspicious pattern detection (10 patterns) ✅
- [x] Output truncation with notice ✅
- [x] FTS query escaping and SQL parameterization ✅

**Remaining gaps** (behind OpenClaw):
- [ ] Sandbox bash execution (Docker, seccomp, AppArmor)
- [x] SSRF protection for web_fetch (private IP blocking, DNS pinning) — `ssrf.rs` ✅
- [ ] Tool call ID validation
- [x] Path traversal prevention — `memory_get` tool has workspace bounds checking ✅
- [ ] Memory file sanitization (currently trusted)

### 6. Audit Logging
- [ ] Log all tool executions, file accesses, and API calls
- [ ] Structured logs for SIEM integration
- [ ] User attribution on all actions

### 7. Data Encryption
- [x] Encrypt session transcripts — `15-encrypt-at-rest` ✅
- [x] Session file permissions 0o600 — `15-session-perms` ✅
- [ ] Encrypt SQLite database (sqlcipher)
- [ ] Key management infrastructure

### 8. Input Validation & Sanitization
- [x] Validate file paths to prevent traversal — `memory_get` tool ✅
- [ ] Sanitize LLM-generated commands
- [ ] Limit file sizes on read/write operations

### 9. Multi-Tenancy
- [ ] Isolate user workspaces
- [ ] Prevent cross-user data access
- [ ] Resource quotas per user/org

## Medium Priority

### 10. Tool Approval Workflow
- [ ] Config has `tools.require_approval` but it's not implemented
- [ ] Add approval flow for sensitive operations
- [ ] Manager approval for certain commands

### 11. Heartbeat/Autonomous Mode Controls
- [ ] Enterprise may want to disable autonomous execution
- [ ] Require approval for self-scheduled tasks
- [ ] Audit trail for autonomous actions

### 12. Content Filtering
- [ ] DLP (Data Loss Prevention) integration
- [ ] Prevent exfiltration of sensitive data via web_fetch
- [ ] Block PII from being sent to LLM providers

### 13. Compliance
- [ ] SOC 2 readiness (logging, access controls, encryption)
- [ ] GDPR/CCPA data handling (retention, deletion)
- [ ] Audit trail for compliance reporting

## Quick Wins for First Enterprise Beta

| Enhancement | Effort | Impact | Status |
|-------------|--------|--------|--------|
| Add API key auth to endpoints | Low | High | ✅ Done |
| Encrypt config file | Low | High | ✅ Done |
| Implement rate limiting | Low | Medium | ✅ Done |
| Restrict CORS origins | Low | Medium | Open |
| Add audit logging | Medium | High | Open |
| Implement tool approval | Medium | High | Open |

---
*Created: 2026-02-04*
*Updated: 2026-03-19 — Marked completed items from GAPS.md (gateway auth, encryption at rest, rate limiting, session perms, path traversal prevention)*
