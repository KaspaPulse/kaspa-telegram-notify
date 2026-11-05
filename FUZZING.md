# Fuzzing

Kaspa Pulse fuzzes the text and wallet-input safety boundary with `cargo-fuzz` and LLVM libFuzzer on Linux.

The `text_safety` target exercises normalization, invisible-character filtering, HTML escaping, log redaction, callback-data redaction, wallet extraction, size validation, and wallet validation with arbitrary byte sequences.

## Reproduce locally

Install the pinned nightly toolchain and cargo-fuzz, then run:

```bash
rustup toolchain install nightly-2026-08-01 --profile minimal
cargo install cargo-fuzz --version 0.13.2 --locked
cargo +nightly-2026-08-01 fuzz run text_safety -- -max_total_time=300 -timeout=5 -max_len=2048
```

Crash artifacts and generated corpora stay under `fuzz/` and are intentionally ignored. Any reproducible crash should become a deterministic regression test before the fix is merged.
