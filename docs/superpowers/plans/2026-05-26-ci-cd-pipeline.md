# CI/CD + 发布流程 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在已有 CI 基础上补充安全检查、发布工作流和版本管理自动化

**Architecture:** 现有 `.github/workflows/ci.yml` 已经有 check/test/lint/build 多平台矩阵，需要：① 添加 `cargo audit` 安全审计步骤，② 创建 release 工作流（tag push 触发），③ 添加 `#[ignore]` 测试治理策略，④ 添加 `cargo deny` 许可证合规和 `cargo outdated` 通知

**Tech Stack:** GitHub Actions, cargo-audit, cargo-deny

---

### Task 1: 添加 cargo-audit 安全审计

**Files:**
- Modify: `.github/workflows/ci.yml`

**Current CI file** (`.github/workflows/ci.yml`):
```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    name: Test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust (ubuntu)
        if: runner.os == 'Linux'
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable

      - name: Install Rust (macOS)
        if: runner.os == 'macOS'
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable

      - name: Install Rust (Windows)
        if: runner.os == 'Windows'
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: ~/.cargo/registry
          key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}

      - name: Cache cargo index
        uses: actions/cache@v4
        with:
          path: ~/.cargo/git
          key: ${{ runner.os }}-cargo-index-${{ hashFiles('**/Cargo.lock') }}

      - name: Cache cargo target dir
        uses: actions/cache@v4
        with:
          path: target
          key: ${{ runner.os }}-cargo-target-${{ hashFiles('**/Cargo.lock') }}

      - name: Check
        run: cargo check

      - name: Test (library)
        run: cargo test --lib

      - name: Test (integration)
        run: cargo test --tests

      - name: Build release
        run: cargo build --release

  lint:
    name: Lint
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable
          components: clippy, rustfmt

      - name: Clippy
        run: cargo clippy -- -D warnings

      - name: Format check
        run: cargo fmt --check
```

- [ ] **Step 1: Add `audit` job to `ci.yml`**

```yaml
  audit:
    name: Security Audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install cargo-audit
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-audit

      - name: Run cargo audit
        run: cargo audit

      - name: Run cargo audit (advisory-only, no error for informational)
        continue-on-error: true
        run: cargo audit --deny warnings
```

Add this as a new job after the `lint` job block. The `--deny warnings` step uses `continue-on-error` to avoid failing on informational advisories while still showing them.

- [ ] **Step 2: Verify audit job**

Run locally to test:
```bash
cargo audit --deny warnings
```
Expected: either pass, or show informational advisories. If there are actual vulnerabilities, they need to be fixed.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add cargo-audit security audit job"
```

---

### Task 2: 添加 cargo-deny 许可证合规检查

**Files:**
- Create: `deny.toml`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Create `deny.toml`**

```toml
# cargo-deny configuration
[advisories]
vulnerability = "deny"
unmaintained = "warn"
notice = "warn"
unsound = "denn"
yanked = "warn"
ignore = []

[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Zlib",
    "CC0-1.0",
    "Unlicense",
    "0BSD",
]
deny = []
copyleft = "deny"
allow-osi-fsf-free = "neither"
default = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"
highlight = "all"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

- [ ] **Step 2: Install and run `cargo-deny` to verify**

```bash
cargo install cargo-deny --locked
cargo deny check licenses
```
Expected: list of licenses found, all should match the allow list.

- [ ] **Step 3: Add deny job to `ci.yml`**

Add to `ci.yml` after the `audit` job:
```yaml
  deny:
    name: License Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install cargo-deny
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-deny

      - name: Run cargo deny
        run: cargo deny check
```

- [ ] **Step 4: Verify CI locally and commit**

```bash
cargo deny check
git add deny.toml .github/workflows/ci.yml
git commit -m "ci: add cargo-deny license and advisory check"
```

---

### Task 3: 添加 Release 工作流

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create release workflow**

Create `.github/workflows/release.yml`:
```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

env:
  CARGO_TERM_COLOR: always
  PROJECT_NAME: rupoo

jobs:
  build-mac:
    name: Build (macOS ARM64)
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable
          target: aarch64-apple-darwin

      - name: Build release
        run: cargo build --release --target aarch64-apple-darwin

      - name: Package binary
        run: |
          cd target/aarch64-apple-darwin/release
          tar czf "${{ env.PROJECT_NAME }}-${{ github.ref_name }}-mac-aarch64.tar.gz" rupoo
          mv "${{ env.PROJECT_NAME }}-${{ github.ref_name }}-mac-aarch64.tar.gz" "${{ github.workspace }}/"

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: rupoo-mac-aarch64
          path: rupoo-${{ github.ref_name }}-mac-aarch64.tar.gz

  build-linux:
    name: Build (Linux x86_64)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable

      - name: Build release
        run: cargo build --release

      - name: Package binary
        run: |
          cd target/release
          tar czf "${{ env.PROJECT_NAME }}-${{ github.ref_name }}-linux-x86_64.tar.gz" rupoo
          mv "${{ env.PROJECT_NAME }}-${{ github.ref_name }}-linux-x86_64.tar.gz" "${{ github.workspace }}/"

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: rupoo-linux-x86_64
          path: rupoo-${{ github.ref_name }}-linux-x86_64.tar.gz

  create-release:
    name: Create Release
    needs: [build-mac, build-linux]
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4

      - name: Download all artifacts
        uses: actions/download-artifact@v4
        with:
          path: artifacts

      - name: Generate changelog
        id: changelog
        run: |
          git log --oneline --no-decorate $(git describe --tags --abbrev=0 2>/dev/null || git rev-list --max-parents=0 HEAD)..HEAD > changelog.txt
          echo "## What's Changed" > release-notes.md
          cat changelog.txt | sed 's/^/- /' >> release-notes.md

      - name: Create Release
        uses: softprops/action-gh-release@v2
        with:
          name: ${{ github.ref_name }}
          body_path: release-notes.md
          files: |
            artifacts/rupoo-mac-aarch64/rupoo-*-mac-aarch64.tar.gz
            artifacts/rupoo-linux-x86_64/rupoo-*-linux-x86_64.tar.gz
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release workflow for tag pushes"
```

---

### Task 4: 添加 `#[ignore]` 测试治理策略

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `Cargo.toml` (dev-dependencies)

**Problem:** Integration tests that need LLM API keys should be `#[ignore]`d in CI. Currently `cargo test --tests` runs all tests including those needing external services.

- [ ] **Step 1: Add a helper module for conditional test skipping**

Create `tests/common/mod.rs`:
```rust
/// Returns true if the CI environment has the given API key configured.
/// Tests that need a live LLM API should use `#[ignore]` + this check.
pub fn has_api_key(provider: &str) -> bool {
    let var = format!("RUPOO_TEST_KEY_{}", provider.to_uppercase());
    std::env::var(var).is_ok_and(|v| !v.is_empty())
}

/// Skip message for integration tests.
pub fn skip_reason(provider: &str) -> String {
    format!("skipped: set RUPOO_TEST_KEY_{} to run", provider.to_uppercase())
}
```

- [ ] **Step 2: Update CI to skip `[ignore]`d tests and add optional full test**

In `ci.yml`, update the test steps:
```yaml
      - name: Test (library)
        run: cargo test --lib

      - name: Test (integration, excluding ignored)
        run: cargo test --tests -- --skip=needs_api_key
```

Also add a conditional full-test job:
```yaml
  full-test:
    name: Full Test (with API keys)
    runs-on: ubuntu-latest
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4

      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable

      - name: Test all (including ignored)
        run: cargo test --tests
        env:
          RUPOO_TEST_KEY_ANTHROPIC: ${{ secrets.RUPOO_TEST_KEY_ANTHROPIC }}
```

- [ ] **Step 3: Commit**

```bash
git add tests/common/mod.rs .github/workflows/ci.yml
git commit -m "ci: add test governance with #[ignore] handling"
```

---

### Task 5: 添加版本检查和更新提醒

**Files:**
- Create: `.github/workflows/deps.yml`

- [ ] **Step 1: Create weekly dependency check workflow**

Create `.github/workflows/deps.yml`:
```yaml
name: Dependency Check

on:
  schedule:
    # Run Monday morning UTC
    - cron: '7 9 * * 1'
  workflow_dispatch:

env:
  CARGO_TERM_COLOR: always

jobs:
  outdated:
    name: Check outdated crates
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable

      - name: Install cargo-outdated
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-outdated

      - name: Check outdated
        run: cargo outdated --exit-code 1
        continue-on-error: true
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/deps.yml
git commit -m "ci: add weekly dependency outdated check"
```

---

## Execution Order

1. Task 1 (audit) → 可以在不修改代码的情况下验证效果
2. Task 2 (deny) → 并行于 Task 1
3. Task 3 (release) → 依赖前两个 task 但不阻塞
4. Task 4 (test governance) → 可与 Task 3 并行
5. Task 5 (weekly deps) → 最后，辅助性任务
