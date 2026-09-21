# Designing the Auth Plane

**How a local-first agent earns a mobile companion without acquiring an identity provider**

| Field | Value |
|-------|-------|
| Version | 1.0 |
| Date | September 20, 2026 |
| Author | Yi / LocalGPT |
| Status | Design — not implemented |
| Motivation | [../ecosystem/orca-architecture-and-rust-feasibility.md](../ecosystem/orca-architecture-and-rust-feasibility.md) §4.2 |

---

## 1. The Question Worth Asking First

Orca open-sourced its entire relay fleet — director, cells, push gateway, Terraform — and kept
`login.onorca.dev` private. That service mints the relay token. Without it the open relay is a
splicing proxy nobody is admitted to.

The instinct is to conclude "so we need an auth service too." That is the wrong lesson. The
right one is: **an auth plane is the tax you pay for a rendezvous service, and most of what a
local-first agent does needs no rendezvous at all.**

Work out where a third party actually mediates:

| Path | Who mediates | Identity needed |
|------|--------------|-----------------|
| CLI → local daemon | nobody, same machine | kernel peer credentials |
| Desktop → daemon over LAN | nobody, both endpoints are the user's | a pairing secret |
| Client → daemon over SSH | the user's own SSH credentials | none of ours |
| **Phone → daemon across NAT** | **a relay we operate** | **the only real question** |

One row. The auth plane exists for one row, and its job is narrow: let a relay decide *which
two sockets to splice* without learning who either party is or what they say.

---

## 2. Principles

1. **Authenticate where a third party mediates, nowhere else.** Every layer that can prove
   identity from the operating system should do so and stop there.
2. **Device identity, not user identity.** The primitive is a keypair on a machine, not an
   email address. Accounts are a product feature; pairing is not.
3. **The relay is untrusted by construction.** It sees ciphertext and opaque slot ids. Nothing
   it stores is worth stealing, so nothing it stores needs an account system to protect.
4. **Pairing is a key exchange wearing a login's clothes.** The QR code is an out-of-band
   authenticated channel — a camera pointed at a screen the user controls.
5. **Revocation is local and authoritative.** The host decides which devices may reach it.
   Losing the relay must never mean losing control of your own daemon.
6. **Degrade to self-hosted, never to bricked.** If our relay disappears, LAN and SSH paths
   must keep working untouched.

---

## 3. Four Layers

### Layer 0 — Local: the OS is the auth plane

Already implemented. `crates/bridge` binds a Unix socket and `peer_identity.rs` reads
`SO_PEERCRED` / `getpeereid`; the server rejects any connection whose UID is not our own. The
endpoint-ownership work additionally publishes the socket at mode `0600`, so the check is
defence in depth rather than the only gate.

Nothing cryptographic belongs here. A process that can forge `SO_PEERCRED` already owns the
machine.

### Layer 1 — Device identity

A long-lived Ed25519 keypair per installation, at
`~/.local/share/localgpt/localgpt.device.key` — the path already exists for other uses.

- The **device ID** is a truncated hash of the public key, rendered in a readable form so a
  user can compare it across a screen and a phone.
- The private key never leaves the machine and is used only to sign challenges.
- It is not an account. Two machines belonging to one person are two devices, deliberately:
  compromise of one must not imply the other.

### Layer 2 — Pairing: host ↔ client, with no server involved

The host displays an offer; the client's camera reads it. That optical hop is the
authentication — it proves physical presence at the host's screen.

```
localgpt://pair?v=1&c=<base64url>
```

The payload carries a protocol version, the host's ephemeral X25519 public key, its device
fingerprint, candidate endpoints (LAN address, relay slot), a nonce, and an expiry. Keep it
small enough to scan reliably; refuse to decode anything oversized before parsing it.

The handshake itself should be **Noise (`snow` crate), pattern `XX`** rather than anything
hand-rolled. Noise gives mutual authentication, forward secrecy, and a key schedule that has
been analysed by people who do that for a living. The output is a long-term pairing record on
both sides:

```
PairingRecord { peer_device_id, peer_public_key, shared_secret, label, created_at, last_seen }
```

Rules that matter more than the cryptography:

- **Expire the offer in ~60 seconds** and make it single-use. A QR photographed over a
  shoulder must be worthless by the time it is used.
- **Confirm on the host.** After the handshake, show the peer's fingerprint on *both* screens
  and require a tap on the host. This is what defeats a relay that tries to splice an attacker
  into a pairing.
- **Rate-limit the manual short-code path** hard — it is the weak channel and needs an attempt
  budget, not just an expiry.
- The relay, if involved at all, carries opaque frames through this exchange and learns
  nothing.

### Layer 3 — Rendezvous admission: the only part that is a service

The relay answers exactly one question: *may these two connections be spliced?* It should be
able to answer without an account database.

```
Host                          Relay                        Phone
 │── hello(device_pubkey) ────▶│                             │
 │◀── challenge(nonce) ────────│                             │
 │── sign(nonce) ─────────────▶│  verify against the key     │
 │◀── slot_token(slot, ttl) ───│  the host itself presented  │
 │                             │                             │
 │                             │◀── present(slot, proof) ────│
 │                             │  proof = MAC over the slot  │
 │                             │  under the pairing secret   │
 │◀════════ spliced, end-to-end encrypted ═══════════════════▶│
```

What the relay stores: `slot_id → (device fingerprint, socket, expiry)`. No email, no
password, no account row. A slot is a random opaque id with a short TTL.

The phone's proof is derived from the **pairing secret**, which the relay does not have — so
the relay can verify that *someone* presenting a valid MAC arrived, only because the host
told it what to expect for that slot. It still cannot mint one itself, and it cannot read the
session.

**This is the design that makes `login.onorca.dev` unnecessary.** Admission is proven by
possession of keys the user already has, not by an account the vendor issues.

---

## 4. When You Actually Need Accounts

Three product features, and only these three, genuinely require an identity provider:

| Feature | Why an account is unavoidable |
|---------|-------------------------------|
| Settings sync across devices | Something must decide which devices are "yours" |
| Team or org sharing | Membership is inherently a server-side relation |
| Billing | Payment is an identity |

LocalGPT needs none of them today. If one arrives later, the containment rule is:

> The identity service issues capability tokens for the rendezvous plane and nothing else.
> It never holds pairing secrets, session keys, or message content. Deleting it must degrade
> the product to self-hosted pairing, not break it.

Build the account plane, if ever, as a **strictly additive** layer over Layer 3. The moment
identity becomes load-bearing for the data path, self-hosting dies and the open-source claim
becomes decorative — which is precisely the position Orca is in.

---

## 5. Threat Model

| Threat | Mitigation | Layer |
|--------|-----------|-------|
| Malicious or compromised relay | E2EE; relay holds no key material; opaque slot ids | 2, 3 |
| Relay operator correlates users | Slots are random and short-lived; no account identifiers | 3 |
| QR observed over a shoulder | ~60s expiry, single use, host-side confirmation of fingerprint | 2 |
| Short-code brute force | Attempt budget per offer, then invalidate the offer entirely | 2 |
| Replay of a captured handshake | Noise nonces; per-session keys; slot TTL | 2, 3 |
| Hostile local process | `SO_PEERCRED` same-UID check; socket mode `0600` | 0 |
| Socket hijack by a racing daemon | Endpoint-ownership protocol (`crates/bridge/src/endpoint.rs`) | 0 |
| Lost or stolen phone | Host-side revocation of the pairing record; no server round-trip | 1, 2 |
| Our relay disappears | LAN and SSH paths unaffected; relay is optional by design | 3 |

Revocation deserves emphasis: it belongs on the **host**, in the pairing record store. A user
who loses a phone must be able to cut it off from the machine itself, offline. Any design
where revocation requires reaching a vendor service has put the vendor between a user and
their own computer.

---

## 6. Crates

| Concern | Crate | Status |
|---------|-------|--------|
| Handshake | `snow` (Noise `XX`) | to add |
| Signatures | `ed25519-dalek` | already in tree |
| Key agreement | `x25519-dalek` | to add |
| AEAD | `chacha20poly1305` | already in tree |
| Relay service | `axum` + `tokio-tungstenite` | already in tree |
| Relay state | `sqlx` or `tokio-postgres` | to add, service-side only |
| QR rendering | `qrcode` | to add, host-side only |

Prefer `snow` over assembling a handshake from the dalek primitives directly. The primitives
are correct; the *protocol* around them is where hand-rolled designs fail, and Noise is a
specified answer to exactly this shape of problem.

---

## 7. Staging

| Stage | Scope | Depends on a service? |
|-------|-------|----------------------|
| 0 | Local socket, peer credentials, `0600` | **done** |
| 1 | Device key, pairing records, Noise handshake over LAN | no |
| 2 | Revocation UI and fingerprint confirmation | no |
| 3 | Rendezvous relay with slot admission | yes — self-hostable |
| 4 | Accounts, only if a listed feature in §4 arrives | yes |

Stages 1 and 2 deliver a working mobile companion on a LAN with no server at all, and they are
where the design should be proven. Stage 3 adds reach; it does not add trust.
