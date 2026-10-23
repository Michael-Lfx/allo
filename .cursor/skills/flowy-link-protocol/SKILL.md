---
name: flowy-link-protocol
description: Protocol rules for Flowy Link on the allo PC host. Use when adding routes, auth, pairing, WebSocket events, or cloud wake for the mobile client.
---

# Flowy Link protocol

The PC process remains the authority. Mobile clients are installation-owner peers with `aud=nomifun-link`, not copies of the desktop local-trust secret.

## Routes

Every product route string under `crates/backend` must classify in `flowy-link-kernel::surface`. Classes:

- `link-ready`: a paired phone may eventually invoke it; it takes effect in this process.
- `step-up`: the phone may request it; the desktop must confirm. Use this for credential minting and host file registration.
- `public-asset`: no owner session required.
- `external-channel`: companion token, MCP, or OAuth front door. Not a Link session.
- `robot-device`: the physical robot channel.

A new `/api/...` path that matches no prefix fails `cargo test -p flowy-link-kernel`.

Do not move the plaintext `:25808` listener to TLS. Link's port is `25818`.

## Wake

Cloud wake defaults on. The payload fields are only `v`, `kind`, `hostFingerprint`, `conversationId`, `confirmationId`, `sentAtMs`. Reject unknown JSON fields. Kinds are `confirmation.pending`, `turn.completed`, `turn.failed`.

## Data

Paired phones live in `linked_devices`. Do not put conversation bodies in FlowyClaw. Revocation of one device must not rotate the desktop JWT secret.
