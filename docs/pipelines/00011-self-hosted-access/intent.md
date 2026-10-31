---
status: draft
---

# Intent 00011 — Self-hosted access from other devices

## Problem

A self-hosted instance listens on localhost only. An operator wants to reach it from a phone or another machine. That might be through a LAN address, a private network host name such as a Tailscale name, or their own domain.

Sign-in depends on cookies. A browser rejects a cookie set for a different host, or one marked secure over plain HTTP. The operator is then stuck at sign-in with no error that says why. Connecting a channel also redirects the browser back to Pipewhere, so the redirect must return to the address the operator is using.

## Proposed outcome

Following the README, an operator can serve their instance at an address they choose. From another device they can then sign in, use the web UI, and connect channels. An address that is misconfigured produces an error naming the setting to fix.

## Affected users and systems

Self-hosting operators; Web, Auth, the provider connection flow, and the Compose configuration.

## Constraints

- Reaching the instance at another address changes configuration, not architecture, per [infrastructure intent](../00001-infrastructure/intent.md).
- A single operator on one machine keeps working with no extra setup.

## Open questions

- Does Pipewhere ship a way to serve HTTPS, or does the operator bring their own proxy?
