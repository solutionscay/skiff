---
name: release
description: Cut a Skiff release. Use when the user asks to create, cut, or publish a release (for example "create the 0.6.2 release"). Bumps versions, tags, waits for CI, and publishes the draft.
---

# Release Skiff

A `v*` tag triggers `.github/workflows/release.yml`. CI builds the Linux packages
(AppImage, deb, rpm) and the macOS DMG. It attaches them to a **draft** release.

## Steps

1. Get the version from the operator. Use the form `X.Y.Z`.
2. Check that the working tree is clean and you are on `main`.
3. Set the version in `Cargo.toml` (`[workspace.package]`) and `apps/desktop/package.json`.
4. Refresh the lockfile: `cargo update --workspace --offline`.
5. Commit: `git commit -am "release: prepare vX.Y.Z"`.
6. Tag and push: `git tag vX.Y.Z && git push origin main vX.Y.Z`.
7. Wait for the run: `gh run watch` (about 10 minutes).
8. Check the assets: `gh release view vX.Y.Z --json isDraft,assets`.
   Expect `.AppImage`, `.deb`, `.rpm` and `.dmg`.
9. Publish only when the operator says so: `gh release edit vX.Y.Z --draft=false --latest`.

## Rules

- Deleting old tags or releases needs explicit operator approval.
- If CI fails, report the failing job. Do not re-tag without asking.
