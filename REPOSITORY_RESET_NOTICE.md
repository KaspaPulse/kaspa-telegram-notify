# Remote main replacement and running-production continuity

On **9 November 2026** the owner-approved local source replaced the old
remote `main` branch tip, without deleting the repository itself.

The verified live Kaspa Pulse **OP092** source was built from actual commit
`0c4650a5ff89f59c5ba9402085ffe6ecd4e1c5a8`. A subsequent
documentation-only commit is not a newly deployed production executable.

This is a **source-only repository**: no actual Telegram token,
database password, private user/chat data, wallet secrets, production dump,
host credentials or runtime state are to be committed. GitHub Actions is
disabled in repository settings and workflow files were removed.
Dependabot automatic updates remain disabled.

Before the remote ref replacement, a private local Git bundle was verified
containing both prior remote and approved production source revisions.
It is not uploaded to GitHub and should be retained for rollback.

**Do not claim cryptographic erasure:** old Git ancestors may still be
reachable through the preserved production source, and forks, GitHub
retention, caches and external clones may retain copies. Production still
runs separately from GitHub and is not deployed from pushes or releases.
