## Summary

<!-- Describe the change and its motivation. Link to the related issue if any. -->

## Type of change

- [ ] Bug fix (non-breaking)
- [ ] New feature (non-breaking)
- [ ] Breaking change (existing behavior changes)
- [ ] Docs / CI / tooling
- [ ] Dependency upgrade

## Checklist

- [ ] `cargo clippy --all-targets -- -D warnings` passes (zero warnings)
- [ ] `cargo fmt --check` passes
- [ ] `cargo test` passes (including new tests for the change)
- [ ] `src-ui`: `pnpm test && pnpm build` passes if the GUI is affected
- [ ] No secrets / API keys in code or config
- [ ] CHANGELOG.md updated if user-facing behavior changed

## Test plan

<!-- How did you verify this change? Include commands and expected results. -->

## Screenshots (optional)

<!-- For UI changes, include before/after screenshots. -->
