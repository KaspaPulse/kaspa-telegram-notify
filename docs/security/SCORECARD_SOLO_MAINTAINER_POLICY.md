# OpenSSF Scorecard solo-maintainer policy

Kaspa Pulse is currently maintained by one direct human GitHub collaborator.
The repository therefore separates two outputs from the same OpenSSF Scorecard policy:

1. **Canonical full Scorecard** — all checks remain enabled and are published to OpenSSF.
2. **Actionable Code Scanning SARIF** — only findings that this repository can remediate
   through source, workflow, dependency, or repository configuration changes are surfaced
   as GitHub Code Scanning alerts.

The full Scorecard remains the authoritative governance posture. Filtering a result from
Code Scanning does not turn that check into PASS and does not change the published score.

## Solo-maintainer governance exclusions

The Code Scanning SARIF excludes exactly these Scorecard rule IDs:

- `CodeReviewID`
- `BranchProtectionID`
- `CIIBestPracticesID`

These are tracked as governance/external-participation limitations in GitHub issue #47.
They must not be represented as independent human review, certification, or 10/10 controls
while the required people or external certification do not exist.

## Why this is not alert suppression

The workflow still runs the complete official Scorecard and publishes the complete result.
A second non-publishing run produces SARIF for GitHub Code Scanning. The repository-owned
filter removes only the three rule IDs above; every unknown or future finding is retained.
The filter is covered by a contract test and fails closed on malformed SARIF.

The main branch protection that is practical for a solo maintainer remains enforced:
PR-only changes, strict required status checks, up-to-date branches, no force-push,
no deletion, admin enforcement, and review-thread resolution.

## Re-enable governance alerts when conditions change

Remove an exclusion when its underlying control becomes actionable. In particular:

- Re-enable `CodeReviewID` and strengthen review rules when an independent human reviewer
  is available and real human-reviewed changes can be accumulated.
- Re-enable `BranchProtectionID` when required human approvals/CODEOWNERS approval can be
  enforced without deadlocking owner and Dependabot changes.
- Re-enable `CIIBestPracticesID` when an OpenSSF Best Practices project is enrolled and
  the repository chooses to treat badge tier progress as an actionable Code Scanning gate.

Issue #47 remains the durable tracker for these external governance conditions.
