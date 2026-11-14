# Releasing

**Status:** current procedure. Two tags release three things: `vX.Y.Z`
publishes the crates and the npm package, `desktop-vX.Y.Z` (the same
version) drafts the desktop app's release.

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
git push origin main vX.Y.Z                         # release.yml and npm.yml publish
```

The tag push runs `.github/workflows/release.yml`: it checks the tag
against the version, asks crates.io which crates are already up
(`cargo xtask release-plan`, so a rerun finishes a partial publish), builds
the rest from their packages and uploads them in dependency order, through
Trusted Publishing. The same push runs `npm.yml`, below; neither waits for
the other.

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

## The npm package

`localgpt` on npm is the command line, prebuilt: one package holding the
binary for macOS (Apple silicon and Intel), Linux (x64 and arm64, glibc 2.35
or newer) and Windows (x64), and a launcher that starts the one for the
computer it is on. It is built without the audio models (`cargo install
localgpt --features audio` has them), so it is the same on all five.

`.github/workflows/npm.yml` builds the five, packs them (`cargo xtask
npm-package`, which refuses a tag that is not the workspace's version),
installs the tarball on each of the five platforms and runs what it
installed, and then publishes that tarball. A `vX.Y.Z` tag publishes; a run
by hand publishes only when told to:

```bash
gh workflow run npm.yml                                # the rehearsal: everything but the publish
gh workflow run npm.yml --ref vX.Y.Z -f publish=true   # a release whose own run failed
```

Run the rehearsal before tagging: it is the only place the five builds and
the package are tried. Its tarball is the run's `npm-package` artifact.

- Publishing is npm's trusted publishing: no token in the repository, and
  npm records where the package was built. On npmjs.com, in the package's
  Settings > Trusted Publisher > GitHub Actions: organization `localgpt-app`,
  repository `localgpt`, workflow `npm.yml`, environment `npm`.
- npm sets that up only for a package that exists, so the first release
  goes up with a token: a granular access token with read and write on all
  packages that skips 2FA and expires in days, as the `npm` environment's
  `NPM_TOKEN` secret (`gh secret set NPM_TOKEN --env npm`). Delete the token
  and the secret once the package is up. npm asks the trusted publisher
  first and falls back to the token, so the workflow is the same for both.
- npm drops a trusted publisher that has not published within two days of
  being set up. Set it up in the two days before the second release, not
  after the first.
- A version on npm is there for good: a mistake is a new patch version.
  `npm deprecate localgpt@X.Y.Z "<why>"` warns whoever installs the old one.
- A pre-release (`vX.Y.Z-rc.1`) is published as `next`, not `latest`.

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
