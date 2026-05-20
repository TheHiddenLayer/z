# Automated GitHub Releases Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fully automate Rust package version PRs, release tags, GitHub Releases, and downloadable binary assets.

**Architecture:** `release-plz` owns version/changelog PRs and git-only release tagging. The existing CI workflow owns cross-platform binary builds and uploads those binaries to the release created for each `v*` tag.

**Tech Stack:** GitHub Actions, release-plz, GitHub CLI, Rust/Cargo.

---

### Task 1: Configure Release Metadata

**Files:**
- Create: `release-plz.toml`

- [ ] **Step 1: Add release-plz config**

```toml
[workspace]
git_only = true
release_always = false
pr_branch_prefix = "release-plz-"
pr_labels = ["release"]
semver_check = false
```

- [ ] **Step 2: Validate TOML**

Run: `python3 -c 'import tomllib; tomllib.load(open("release-plz.toml","rb"))'`
Expected: command exits 0 with no output.

### Task 2: Add Release PR Automation

**Files:**
- Create: `.github/workflows/release-plz.yml`

- [ ] **Step 1: Create workflow**

Create a workflow that runs on pushes to `master`, runs `release-plz release` for merged release PRs, runs `release-plz release-pr` for unreleased changes, and enables auto-merge on the release PR with `gh pr merge --auto --merge --delete-branch`.

- [ ] **Step 2: Validate token assumptions**

Use `secrets.RELEASE_BOT_TOKEN` for release-plz and GitHub CLI so bot-created PRs/tags trigger downstream workflows.

### Task 3: Publish Downloadable Binaries

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Restrict binary packaging to version tags**

Change the `release` job guard to `startsWith(github.ref, 'refs/tags/v')`.

- [ ] **Step 2: Add GitHub Release upload job**

Download matrix artifacts, generate `sha256sums.txt`, create a release if a manual tag did not already create one, and upload binaries/checksums with `gh release upload --clobber`.

- [ ] **Step 3: Include publish job in CI aggregate**

Add `publish-release` to `ci-success.needs` so asset upload failures fail the overall workflow.

### Task 4: Verify

**Files:**
- Validate: `.github/workflows/ci.yml`
- Validate: `.github/workflows/release-plz.yml`
- Validate: `release-plz.toml`

- [ ] **Step 1: Parse YAML and TOML locally**

Run: `ruby -e 'require "yaml"; ARGV.each { |f| YAML.load_file(f) }; puts "ok"' .github/workflows/ci.yml .github/workflows/release-plz.yml`

Run: `python3 -c 'import tomllib; tomllib.load(open("release-plz.toml","rb"))'`

- [ ] **Step 2: Run Rust static checks**

Run: `cargo fmt --all --check`

Run: `cargo check --locked --workspace`
