# CI Script Dependency Policy

Policy: scripts under `scripts/ci/` must rely only on POSIX shell and common base tools (`coreutils`, `grep`, `awk`, `sed`) unless the GitHub Actions workflow explicitly installs additional tools.

This prevents CI breakage from missing nonstandard binaries on runners.

## Example

Bad (implicit extra dependency):

```bash
rg -n "QSL_AWS_UPDATE_RESULT" /tmp/aws-wrapper.log
```

Good (portable default):

```bash
grep -n "QSL_AWS_UPDATE_RESULT" /tmp/aws-wrapper.log
```
