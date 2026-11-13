# Releasing

**Status:** current procedure. Two tags release two things: `vX.Y.Z`
publishes the crates, `desktop-vX.Y.Z` (the same version) drafts the desktop
app's release.

## The crates

cargo-release with one shared version (`[workspace.metadata.release]`).
Pass `--workspace` to every step that selects packages (`version`,
`publish`, `tag`, `push`): the root's `default-members` is the CLI alone,
so without it cargo-release selects only `localgpt`. `commit` takes no
package flags and always commits the whole workspace. Edit `CHANGELOG.md`
(Unreleased becomes `## [X.Y.Z] - date`, plus the links at the bottom) in
its own commit first.

```bash
cargo release version X.Y.Z --workspace --execute   # bumps every crate and the internal requirements
cargo release commit --execute                      # the "chore: Release" commit
git tag -s vX.Y.Z -m vX.Y.Z                         # signed, like every v tag
git push origin main vX.Y.Z                         # .github/workflows/release.yml publishes
```

The tag push runs `.github/workflows/release.yml`: it checks the tag
against the version, asks crates.io which crates are already up
(`cargo xtask release-plan`, so a rerun finishes a partial publish), builds
the rest from their packages and uploads them in dependency order, through
Trusted Publishing. In parallel (`build-cli` → `publish-npm`), it builds the
`localgpt` CLI across macOS (arm64, x64), Linux (x64, arm64) and Windows
(x64), assembles the `@localgpt/cli-<os>-<arch>` and `localgpt` npm packages
with `cargo xtask npm-package`, and publishes them to npm when `NPM_TOKEN` is
set.

- A crate published for the first time needs the `CARGO_REGISTRY_TOKEN`
  secret, since crates.io can only trust a workflow for a crate that
  exists. `cargo xtask trust-release` then makes every published crate
  trust `release.yml`.
- crates.io limits how many new crates go up at once, and cargo waits only
  a minute for an upload to reach the index before publishing what depends
  on it. When a publish fails that way, rerun the failed job: it picks up
  the rest.
- Members with `publish = false` (app, doc, verse, relay, previs-web) stay
  off crates.io.
- `cargo publish --workspace --dry-run` is the local check.

## The desktop app

`desktop-vX.Y.Z` runs `.github/workflows/desktop.yml`, which builds
`LocalGPT.app` (including `Contents/MacOS/localgpt`) for Apple silicon and
Intel with `cargo xtask bundle --dmg`
and drafts a GitHub release with the two `.dmg` files and the Homebrew cask
(which installs `LocalGPT.app` and links `localgpt` onto `PATH`).
A hyphen after the version makes a pre-release; a manual run keeps the
`.dmg` files as artifacts; signing and notarization run when their secrets
are set. [`apps/app-desktop/README.md`](../apps/app-desktop/README.md) has
the secrets and the steps after the draft.
