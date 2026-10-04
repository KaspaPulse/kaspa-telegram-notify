# Active Security Advisory Exceptions

This document is the human-readable view of the repository's current reviewed dependency exceptions. The canonical structured authority is `proof/policy.toml`; scanner-specific ignore sets in `osv-scanner.toml`, `.cargo/audit.toml`, and `deny.toml` are machine-verified against it.

Last automated review: **2026-10-04**

## Policy

- Exceptions are advisory-specific, time-bounded, and fail closed when expired or inconsistent.
- A scanner ignore is invalid unless the same advisory exists in the canonical structured policy with a current review, justification, reachability assessment, remediation trigger, and expiry.
- Resolved advisories are removed from this current-state view and remain available through Git history.
- A passing exception policy does not replace independent vulnerability scanning.
- Re-review is required whenever the relevant dependency path changes and no later than the canonical expiry date.
- Current dependency versions and revisions come from `Cargo.toml` and `Cargo.lock`, not from prose in this document.

Current validation:

```bash
cargo xtask security advisories --max-age-days 45
cargo audit
cargo deny check
cargo machete
```

## Current managed findings

### RUSTSEC-2025-0052 — async-std

Classification: transitive, runtime graph, unmaintained upstream dependency.

Current path: `async-std 1.13.2 → workflow-core 0.18.0 → pinned rusty-kaspa 2.1.0 graph`.

The application does not select async-std directly. Remove this exception when the pinned upstream path no longer resolves it or a compatible maintained path becomes available.

Expiry: **2026-10-15**.

### RUSTSEC-2021-0145 — atty unsoundness

Classification: transitive, platform-limited unsoundness.

Current path: `atty 0.2.14 → hexplay 0.3.0 → pinned Kaspa/workflow graph`.

The published unsoundness is Windows-specific while production qualification is Linux. Remove this exception when the atty path disappears or a compatible maintained replacement is adopted upstream.

Expiry: **2026-10-15**.

### RUSTSEC-2024-0375 — atty unmaintained

Classification: transitive, runtime graph, unmaintained upstream dependency.

Current path: `atty 0.2.14 → hexplay 0.3.0 → pinned Kaspa/workflow graph`.

There is no direct application dependency. Remove the exception when the upstream graph no longer resolves atty.

Expiry: **2026-10-15**.

### RUSTSEC-2024-0388 — derivative unmaintained

Classification: transitive dependency through the Kaspa/Arkworks graph.

Current path: `derivative 2.2.0 → ark-crypto-primitives 0.6.0 → ark-groth16 0.6.0 → kaspa-txscript 2.1.0`.

The application does not select derivative directly. Remove the exception when the pinned Kaspa/Arkworks path no longer resolves it.

Expiry: **2026-10-15**.

### RUSTSEC-2024-0384 — instant unmaintained

Classification: transitive, runtime graph, unmaintained upstream dependency.

Current path: `instant 0.1.13 → workflow-core 0.18.0 → pinned rusty-kaspa 2.1.0 graph`.

The application does not select instant directly. Remove the exception when the upstream workflow graph no longer resolves it.

Expiry: **2026-10-15**.

### RUSTSEC-2024-0436 — paste unmaintained

Classification: transitive/build graph dependency.

Current path includes pinned Kaspa RPC/notify crates and the kaspa-txscript/RISC0 dependency graph.

The application does not select paste directly. Remove the exception when the pinned upstream graph no longer resolves it.

Expiry: **2026-10-15**.

### RUSTSEC-2024-0370 — proc-macro-error unmaintained

Classification: compile-time transitive dependency.

Current path: `proc-macro-error 1.0.4 → kaspa-rpc-macros/workflow-core-macros → pinned Kaspa/workflow graph`.

There is no direct application dependency. Remove the exception when upstream macros migrate away from proc-macro-error.

Expiry: **2026-10-15**.

### RUSTSEC-2026-0173 — proc-macro-error2 unmaintained

Classification: compile-time transitive dependency.

Current path: `proc-macro-error2 2.0.1 → aquamarine 0.6.0 → teloxide 0.17.0 → kaspa-pulse`.

The application does not select proc-macro-error2 directly. Remove the exception when Teloxide/Aquamarine replace or remove it.

Expiry: **2026-10-15**.

### RUSTSEC-2026-0306 — faster-hex AVX2 decode over-read

Classification: transitive runtime dependency with reviewed reachability.

The pinned rusty-kaspa 2.1.0 graph constrains faster-hex to the 0.9.x line. Reviewed selected Kaspa call sites use checked `hex_decode` rather than direct `hex_decode_unchecked`; the fixed 0.10.1 line is outside the current upstream requirement.

Remove this exception immediately when rusty-kaspa permits faster-hex 0.10.1 or newer, or if reachability changes.

Expiry: **2026-10-15**.

## Scanner relationship

The canonical record declares which scanners need an explicit exception. CI verifies exact set equality for Cargo Audit and Cargo Deny and verifies OSV IDs, reasons, and expiry values against the same authority. Unknown, missing, duplicate, stale, or expired exception state fails closed.
