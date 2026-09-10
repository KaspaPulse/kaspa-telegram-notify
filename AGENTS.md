# Repository Execution Policy

<!-- KAS_DNS_ENVIRONMENT_BOUNDARY_V1 -->

This file defines a mandatory execution boundary for all human and automated engineering work in this repository.

## Authoritative environment boundary

- `kas` is the only development host.
- `dns` is the production host and is not a development environment.
- All source edits, dependency resolution, local Git operations, builds, tests, security scans, packaging, and release-artifact creation MUST run on `kas`.
- All branch creation, commits, rebases/merges, pushes, tags, and GitHub coordination initiated from a local host MUST originate from `kas`.
- GitHub Actions remains the repository's remote CI system; `dns` MUST NOT be configured or used as a CI runner.

## Production host prohibition

During development, `dns` MUST NOT be used for:

- Editing repository files.
- Git fetch/pull/checkout/commit/push/tag operations.
- Cargo, Rust, Docker, Node, npm, or other development builds.
- Unit, integration, security, smoke, or CI-equivalent tests.
- Dependency installation or dependency-resolution work for development.
- Generating release artifacts.

## Permitted use of production

`dns` MAY be accessed only after the candidate change is merged and required CI gates have passed, and only for:

- Deploying an already-approved artifact produced outside production.
- Restarting/reloading the application as required by the deployment.
- Post-deployment health, readiness, log, metrics, and functional verification.
- Executing a documented rollback when verification fails.

No source build is permitted on `dns` as part of deployment. Prefer immutable artifacts identified by the merged `main` commit SHA and a cryptographic checksum.

## Required deployment sequence

1. Develop and test on `kas`.
2. Commit and push from `kas`.
3. Open/review the pull request and wait for required GitHub CI gates.
4. Merge to protected `main` only after gates pass.
5. Build/package the deployable artifact on `kas` from the exact merged `main` SHA.
6. Record the artifact SHA-256 and target commit SHA.
7. Access `dns` only for deployment.
8. Preserve a rollback copy before replacement.
9. Deploy the exact approved artifact.
10. Verify service status, health/readiness, logs, and required runtime integrations on `dns`.
11. Roll back immediately if the deployment is materially worse than the pre-deployment baseline.

## Failure and exception handling

If production verification exposes a defect, capture the minimum evidence on `dns`, restore/retain a safe production state, then return to `kas` for diagnosis, code changes, builds, and tests. Do not repair source code in place on production.

This boundary is mandatory even when a task has broad authorization. Authorization to deploy does not convert `dns` into a development host.

If another document or prior conversation conflicts with this file on the `kas`/`dns` execution boundary, this policy governs until it is deliberately changed through protected `main`.