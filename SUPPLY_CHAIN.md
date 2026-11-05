# Kaspa Pulse supply-chain and release controls

**Status: release qualification in progress (5 November 2026).** The clean-root
repository does **not** yet contain a tested production release pipeline.
Publishing a GitHub source tag does not deploy the Telegram bot.

## Build inputs and integrity

- Rust **1.99.1 stable** in `rust-toolchain.toml`.
- `Cargo.lock` committed; release and test builds use `--locked`.
- Rusty Kaspa SDK **2.0.1**, immutably pinned to
  `cfafeb4c093fa37a303f1b9f19c58f986b870ce3`.
- Vendored `teloxide-core` is kept in the source tree with its own license.
- Base images used in the release-candidate Dockerfile are identified with
  immutable image digests; Dockerfile build-policy check passed on ARM64.
  **A full container image build, runtime smoke and vulnerability scan have
  not yet been proven.**
- The CI workflow has **`contents: read`** only. It uses **push**, **pull request**
  and owner-initiated workflow triggers only: **no scheduled dependency updates**,
  no Dependabot configuration, no auto-merge or automatic release.

## Test and security evidence

- Strict Clippy with `-D warnings`, Rustfmt and library tests validated on an
  isolated ARM64 candidate.
- Transactional alert outbox and rollback, wallet queue, runtime DB privilege,
  and functional UI behavior were checked with a disposable PostgreSQL 18
  instance and fixed fixture scope.
- Refreshed `cargo audit` returned zero vulnerability advisories;
  `cargo deny check` passed after explicit documentation of eight upstream
  maintenance notices. Details and residual risk are in
  [SECURITY_ADVISORIES.md](SECURITY_ADVISORIES.md).
- These local results are **not a substitute for successful GitHub CI** on the
  exact candidate commit.

## What this release candidate does not claim

- No published binary, reproducible binary-build equivalence proof, CycloneDX
  SBOM, signed attestation, or SLSA provenance for the post-reset source has
  yet been independently verified.
- No Docker runtime smoke has been verified against this exact candidate.
- The existing GitHub `v2.0.0-rc.1` is **source-only**, with known older CI
  failures; do not repurpose its tag as an artifact authentication anchor.
- Historical GitHub forks and pull-request references may retain old content
  despite the independent root commit. Past exposure remediation is a separate
  privacy task.
- Production `dns` remains unchanged until a separate authorized rollout
  with a retained backup and tested rollback.

## Future signed release policy

Before publishing a stable v2.0.0 release:

1. Build ARM64 and amd64 candidates from the same pinned commit with
   independent hash/revision evidence.
2. Validate isolated PostgreSQL migration/rollback and Telegram wallet
   localization without accessing the production database.
3. Validate container image build, non-root runtime, image digest, health
   behavior, and startup fail-closed conditions.
4. Produce and independently verify SHA-256 hashes, an SBOM and signer
   attestation. Do not claim attestation if no signed artifact exists.
5. Obtain owner approval of final CI and release evidence; only then create a
   stable release tag or authorize a separately controlled production rollout.

The full release checklist is deliberately **fail-closed**: source publication
and good unit-test results alone do not authorize binary distribution.
