---
name: release
description: Cut a Skiff release. Use when the user asks to create, cut, or publish a release (for example "create the 0.6.2 release"). Bumps versions, tags, waits for CI, and publishes the draft.
---

# Release Skiff

A `v*` tag triggers `.github/workflows/release.yml`. CI builds the Linux packages
(AppImage, deb, rpm) and the macOS DMG. It attaches them to a **draft** release.

## Steps

1. Get the app version from the operator. Use the form `X.Y.Z`.
2. Check that the working tree is clean and you are on `main`.
3. Set the app version in `Cargo.toml` (`[workspace.package]`) and `apps/desktop/package.json`.
   Add `<release version="X.Y.Z" date="YYYY-MM-DD"/>` at the top of `<releases>` in
   `apps/desktop/src-tauri/linux/com.solutionscay.skiff.metainfo.xml`.
4. Bump skiffd only if the daemon changed. Find the last tag: `git describe --tags --abbrev=0`.
   If `git diff --quiet <tag> -- crates/skiffd crates/skiff-core` fails and the skiffd
   version is still the one at that tag (`git show <tag>:crates/skiffd/Cargo.toml`), raise
   the patch number of `version` in `crates/skiffd/Cargo.toml` and `crates/skiff-core/Cargo.toml`.
   Keep the two the same. Tell the operator the new skiffd version. A release with no
   skiffd bump asks no user to restart the daemon.
5. Refresh the lockfile: `cargo update --workspace --offline`.
6. Commit: `git commit -am "release: prepare vX.Y.Z"`.
7. Tag and push: `git tag vX.Y.Z && git push origin main vX.Y.Z`.
8. Wait for the run: `gh run watch` (about 10 minutes).
9. Check the assets: `gh release view vX.Y.Z --json isDraft,assets`.
   Expect `.AppImage`, `.deb`, `.rpm` and `.dmg`.
10. Publish only when the operator says so: `gh release edit vX.Y.Z --draft=false --latest`.

## Rules

- Deleting old tags or releases needs explicit operator approval.
- If CI fails, report the failing job. Do not re-tag without asking.
