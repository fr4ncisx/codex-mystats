## Summary

What changed, and why?

## Release note summary

What should WIMU users know? Write `None` for internal-only changes.
Maintainers add the version and finalize `.github/release-notes/vX.Y.Z.md` during release prep.

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --locked`
- [ ] `npm test`
- [ ] `cargo tauri build --no-bundle`

## Review

- [ ] Updated the README if behavior or setup changed; otherwise N/A
- [ ] Added before/after screenshots for UI changes; otherwise N/A
- [ ] Removed credentials, session files, and private Codex usage data from attachments
