# Supply-chain security and release verification

Kaspa Pulse releases are produced by the protected GitHub Actions release workflow in `.github/workflows/release.yml`.

Local builds are qualification artifacts only. They are not project releases and must not substitute for published release bytes. Production deployment must use the exact published artifact after checksum, source identity, signer-workflow, and attestation verification.

## Release artifacts

Starting with `v1.3.1`, every published Linux release contains parallel `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` artifact sets. Each target has:

- a deterministic release archive (`kaspa-pulse-<version>-<target>.tar.gz`);
- its SHA-256 checksum (`*.sha256`);
- a target-specific CycloneDX JSON SBOM (`kaspa-pulse-<version>-<target>.cdx.json`);
- a Sigstore bundle for SLSA build provenance (`*.sigstore.json`);
- a Sigstore bundle for the SBOM attestation (`*.sbom.sigstore.json`);
- an in-toto/SLSA provenance bundle (`*.intoto.jsonl`).

The ARM64 binary is built on GitHub's standard native `ubuntu-24.04-arm` hosted runner using the repository Dockerfile and Buildx without CPU emulation. The native ARM builder verifies that the image is `linux/arm64`, that its OCI revision label equals the exact GitHub commit, and that the extracted ELF embeds the same full source revision. On protected main runs the same native builder creates the deterministic ARM64 archive and target SBOM, generates provenance and SBOM attestations in the builder job, verifies the provenance against the exact source and canonical release workflow, and passes the complete verified bundle to the publication job through an immutable GitHub Actions artifact.

The release workflow uses GitHub OIDC and ephemeral Sigstore signing through GitHub Artifact Attestations. No long-lived release signing private key is stored in the repository.

## Verify a release

Install the GitHub CLI, authenticate to GitHub if required, and download the assets for the release you want to verify.

For `v1.3.1`, choose the target you intend to run:
~~~bash
version="1.3.1"
target="aarch64-unknown-linux-gnu" # or x86_64-unknown-linux-gnu
archive="kaspa-pulse-${version}-${target}.tar.gz"

sha256sum --check "${archive}.sha256"

gh attestation verify "$archive" \
  --repo KaspaPulse/kaspa-telegram-notify \
  --bundle "${archive}.intoto.jsonl" \
  --signer-workflow KaspaPulse/kaspa-telegram-notify/.github/workflows/release.yml
~~~

For deployment, also bind verification to the expected source commit:

~~~bash
expected_source="<40-character release commit>"

gh attestation verify "$archive" \
  --repo KaspaPulse/kaspa-telegram-notify \
  --bundle "${archive}.intoto.jsonl" \
  --source-digest "$expected_source" \
  --signer-workflow KaspaPulse/kaspa-telegram-notify/.github/workflows/release.yml
~~~

Verification must fail closed: do not install or run an artifact when the checksum, source identity, signer workflow, or attestation verification fails.

## Reproducibility controls
The release pipeline:

- builds with the repository-pinned Rust toolchain and `Cargo.lock` using `--locked`;
- preflights both target-specific CycloneDX SBOMs on release-affecting pull requests;
- preflights the ARM64 Docker build on GitHub's native `ubuntu-24.04-arm` runner on release-affecting pull requests;
- embeds the exact GitHub source revision into both release binaries;
- uses deterministic archive order, ownership, timestamp, and gzip metadata;
- generates a target-specific CycloneDX SBOM from the locked dependency graph;
- normalizes each CycloneDX serial number deterministically from repository, commit, version, target, and specification version;
- generates separate SLSA provenance and SBOM attestations for x86_64 and ARM64 before publication, with ARM64 attestations produced in the native ARM builder job;
- downloads and verifies the SLSA provenance bundle for each artifact before creating the GitHub Release;
- publishes both target artifact sets in one GitHub Release tied to the exact workflow commit;
- requires the published release to report `isImmutable=true` and the exact expected asset manifest;
- re-downloads the published ARM64 archive and verifies its checksum, provenance, architecture, and embedded source before the release job succeeds;
- refuses to republish a version tag that already has a release.

## Dependency and advisory controls

Pull requests and scheduled security workflows use independent controls including CodeQL, OSV Scanner, `cargo audit`, `cargo deny`, dependency review, secret scanning, strict Clippy, tests, release builds, and production-container smoke tests.

Reviewed RustSec/OSV exceptions are canonicalized as structured records in `proof/policy.toml`, projected into scanner-specific configuration, and summarized for humans in `SECURITY_ADVISORIES.md`. CI verifies exact scanner consistency and expiry. A passing exception policy does not replace the independent vulnerability scanners.

## Continuous Rust-only and native trust proof

`cargo xtask proof verify` is the required fail-closed repository proof gate. Its schema is pinned to `1.2.0`, and successful verification emits three deterministic JSON artifacts under `target/proof/`:
- `rust-only-proof.json` classifies every tracked/relevant artifact by ownership, role, path, file class, executability, origin, and target relevance; any unknown classification fails the gate;
- `native-dependency-inventory.json` records all production-reachable custom build scripts by digest and classifies the union of Cargo `links`, `-sys` names, and build scripts with compiler/link/process/native-source capability signals. Each candidate has exact locked identity, dependency path, resolved features, discovery signals, target-aware native activation, approval, and validity predicates;
- `supply-chain-proof.json` records the pinned workflow/action policy, dependency/advisory controls, explicit MSRV policy, SBOM/provenance/attestation controls, and input digests.

Owned-source Rust-only proof and transitive-native proof are deliberately separate. Approved third-party native code does not count as owned non-Rust implementation, but a new unclassified native/link/build-script candidate fails closed until the policy is explicitly updated and reviewed. Custom build scripts without native-capability signals are still emitted in the proof so their package identity and build-script digest remain auditable. The security workflow runs this proof gate on pull requests and `main` pushes and publishes the JSON evidence as a CI artifact.

The repository records SLSA provenance predicates and verifies GitHub artifact attestations, but the proof policy sets `slsa_claim = "NOT_ASSERTED"`; no SLSA build level is claimed solely because attestations exist.

## Trust boundary

Only artifacts attached to the canonical GitHub repository release page should be treated as project releases:

https://github.com/KaspaPulse/kaspa-telegram-notify/releases

Do not trust binaries redistributed through unrelated mirrors unless you independently verify their checksum and provenance against the canonical release metadata.
