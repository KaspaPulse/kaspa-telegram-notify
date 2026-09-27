# Supply-chain security and release verification

Kaspa Pulse releases are produced by the protected GitHub Actions release workflow in `.github/workflows/release.yml`.

## Release artifacts

A published release contains the following versioned artifacts:

- the Linux release archive (`*.tar.gz`);
- its SHA-256 checksum (`*.sha256`);
- a CycloneDX JSON SBOM (`*.cdx.json`);
- a Sigstore bundle for SLSA build provenance (`*.sigstore.json`);
- a Sigstore bundle for the SBOM attestation (`*.sbom.sigstore.json`);
- an in-toto/SLSA provenance bundle (`*.intoto.jsonl`).

The release workflow uses GitHub OIDC and ephemeral Sigstore signing through GitHub Artifact Attestations. No long-lived release signing private key is stored in the repository.

## Verify a release

Install the GitHub CLI, authenticate to GitHub if required, and download the assets for the release you want to verify.

For `v1.3.0` on Linux x86_64:

```bash
archive="kaspa-pulse-1.3.0-x86_64-unknown-linux-gnu.tar.gz"

sha256sum --check "${archive}.sha256"

gh attestation verify "$archive" \
  --repo KaspaPulse/kaspa-telegram-notify \
  --bundle "${archive}.intoto.jsonl" \
  --signer-workflow KaspaPulse/kaspa-telegram-notify/.github/workflows/release.yml
```

Verification must fail closed: do not install or run an artifact when the checksum or attestation verification fails.

## Reproducibility controls

The release pipeline:

- builds with the repository-pinned Rust toolchain and `Cargo.lock` using `--locked`;
- uses a deterministic archive order, ownership, timestamp, and gzip metadata;
- generates a CycloneDX SBOM from the locked dependency graph;
- normalizes the CycloneDX serial number deterministically from repository, commit, version, target, and specification version;
- generates SLSA provenance and an SBOM attestation before publication;
- verifies the downloaded SLSA provenance bundle before creating the GitHub Release;
- refuses to republish a version tag that already has a release.

## Dependency and advisory controls

Pull requests and scheduled security workflows use independent controls including CodeQL, OSV Scanner, `cargo audit`, `cargo deny`, dependency review, secret scanning, strict Clippy, tests, release builds, and production-container smoke tests.

Reviewed RustSec/OSV exceptions are advisory-ID-specific, documented in `SECURITY_ADVISORIES.md`, and time-bounded by CI policy. A passing exception policy does not replace the independent vulnerability scanners.

## Continuous Rust-only and native trust proof

`cargo xtask proof verify` is the required fail-closed repository proof gate. Its schema is pinned to `1.0.0`, and successful verification emits three deterministic JSON artifacts under `target/proof/`:

- `rust-only-proof.json` classifies every tracked/relevant artifact by ownership, role, path, file class, executability, origin, and target relevance; any unknown classification fails the gate;
- `native-dependency-inventory.json` records the production-reachable Cargo packages that expose `links` metadata or `-sys` surfaces for both supported Linux targets, including exact locked identity, dependency path, native mechanism, approval, and validity predicates;
- `supply-chain-proof.json` records the pinned workflow/action policy, dependency/advisory controls, explicit MSRV policy, SBOM/provenance/attestation controls, and input digests.

Owned-source Rust-only proof and transitive-native proof are deliberately separate. Approved third-party native code does not count as owned non-Rust implementation, but a new unclassified native/link dependency fails closed until the policy is explicitly updated and reviewed. The security workflow runs this proof gate on pull requests and `main`/`dev` pushes and publishes the JSON evidence as a CI artifact.

The repository currently records SLSA provenance predicates and verifies GitHub artifact attestations, but the proof policy sets `slsa_claim = "NOT_ASSERTED"`; no SLSA build level is claimed solely because attestations exist.

## Trust boundary

Only artifacts attached to the canonical GitHub repository release page should be treated as project releases:

https://github.com/KaspaPulse/kaspa-telegram-notify/releases

Do not trust binaries redistributed through unrelated mirrors unless you independently verify their checksum and provenance against the canonical release metadata.
