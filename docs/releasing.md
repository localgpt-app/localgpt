# Releasing

**Status:** current procedure. Two tags release three things: `vX.Y.Z`
publishes the crates and the npm package, `desktop-vX.Y.Z` (the same
version) drafts the desktop app's release. Homebrew follows both, from the
tap.

No token is kept in the repository. Every upload is a registry trusting
the workflow that makes it, and what a registry cannot trust yet, the first
version of a new name, a person uploads once.

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
git push origin main                                # CI runs; `gh workflow run npm.yml` rehearses this commit
git tag -s vX.Y.Z -m vX.Y.Z                         # signed, like every v tag
git push origin vX.Y.Z                              # release.yml and npm.yml publish
```

Tag once CI and the rehearsal are green on the release commit: what a tag
publishes stays published.

The tag push runs `.github/workflows/release.yml`: it checks the tag
against the version, asks crates.io which crates are already up
(`cargo xtask release-plan`, so a rerun finishes a partial publish), builds
the rest from their packages and uploads them in dependency order, through
Trusted Publishing. The same push runs `npm.yml`, below; neither waits for
the other.

- Trusted Publishing is the whole of the credentials: each crate's
  settings on crates.io name this repository, `release.yml` and the
  `crates-io` environment, and crates.io hands the run a token that lasts
  half an hour. `cargo xtask trust-release` gives every published crate
  that configuration.
- crates.io limits how many new crates go up at once, and cargo waits only
  a minute for an upload to reach the index before publishing what depends
  on it. When a publish fails that way, rerun the failed job: it picks up
  the rest.
- Members with `publish = false` (app, doc, verse, relay, previs-web) stay
  off crates.io.
- `cargo publish --workspace --dry-run` is the local check.

### A crate's first version

crates.io can trust a workflow only for a crate that exists, so a crate it
has never had is not the workflow's to upload. `cargo xtask release-plan`
names it (`by hand`) and whatever depends on it (`waits`); the workflow
uploads the rest and then fails, saying so. A person uploads the first
version with a crates.io token of their own, which never reaches the
repository (crates.io > Account Settings > API Tokens: the `publish-new`
and `trusted-publishing` scopes, the crate's name as the pattern, a day's
expiry):

```bash
git worktree add ../first-version vX.Y.Z && cd ../first-version
read -rs CARGO_REGISTRY_TOKEN && export CARGO_REGISTRY_TOKEN   # paste the token: nothing is shown
cargo publish -p localgpt-<crate>   # packages it, builds the package, uploads it
cargo xtask trust-release           # or the crate's Settings > Trusted Publishing on crates.io
unset CARGO_REGISTRY_TOKEN
```

Then run the workflow's failed job again (`gh run rerun <run> --failed`),
which uploads what waited, and revoke the token.

It need not wait for the release. A new crate that builds against what
crates.io already has (`cargo publish -p localgpt-<crate> --dry-run` says)
can have its first version uploaded from `main` beforehand, at the version
the workspace is at; the release is then the workflow's alone.

## The npm package

`localgpt` on npm is the command line, prebuilt: one package holding the
binary for macOS (Apple silicon and Intel), Linux (x64 and arm64, glibc 2.34
or newer) and Windows (x64), and a launcher that starts the one for the
computer it is on. It is built without the audio models (`cargo install
localgpt --features audio` has them), so it is the same on all five. With
all five in it, it is a 25 MB download.

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

- Publishing is npm's trusted publishing and nothing else: no token in the
  repository, and npm records where the package was built. On npmjs.com, in
  the package's Settings > Trusted Publisher > GitHub Actions: organization
  `localgpt-app`, repository `localgpt`, workflow `npm.yml`, environment
  `npm`, with `npm publish` allowed. Or from a terminal, with npm 11.15 or
  newer, which asks for the second factor:

  ```bash
  npm trust github localgpt --repo localgpt-app/localgpt --file npm.yml --env npm --allow-publish
  ```

- npm drops a trusted publisher that has not published within two days of
  being set up: set it up in the two days before the release that is to
  use it.
- Where the publish job cannot publish (npm has no `localgpt`, or refuses
  the workflow), it fails, and its summary has the commands that publish
  that run's tarball with a person's own login. Run again, it finds the
  version up and checks that npm has that tarball.
- A version on npm is there for good: a mistake is a new patch version.
  `npm deprecate localgpt@X.Y.Z "<why>"` warns whoever installs the old one.
- A pre-release (`vX.Y.Z-rc.1`) is published as `next`, not `latest`.

### The first version on npm

npm sets a trusted publisher up only for a package that exists, so the
first version of all needs a person once, with their own npm login and no
token anywhere. Either way round:

- **Before the tag**, so that the release itself comes from the workflow
  and carries its provenance statement. Publish a placeholder under the
  name (a folder holding a `package.json` with the name, version `0.0.0`
  and nothing to run: `npm publish --access public`), set the trusted
  publisher up, and tag within two days. Afterwards, `npm deprecate
  localgpt@0.0.0 "a placeholder: install the latest"`.
- **After the tag.** The publish job fails, since npm has no `localgpt`,
  and its summary has three commands: download that run's tarball, `npm
  publish` it, and run the job again, which checks that npm has that
  tarball. Set the trusted publisher up in the two days before the next
  release. That first version has no provenance statement: only a workflow
  can make one.

## Homebrew

`brew install localgpt-app/tap/localgpt` installs the command line, on
macOS and Linux, from a formula that takes this computer's binary out of
the npm package: the build the npm workflow installed and ran, with nothing
built or uploaded for Homebrew. `cargo xtask formula <version> <tarball>`
renders it; `npm.yml` renders it beside the tarball (it is in the
`npm-package` artifact), installs it with Homebrew on both Macs and on
Linux, and runs its test.

The tap, [localgpt-app/homebrew-tap](https://github.com/localgpt-app/homebrew-tap),
holds `Formula/localgpt.rb`, and `Casks/localgpt.rb` once a desktop release
is notarized. A tap is a Git
repository, not a registry, so nothing can be trusted to publish to it:
what the release repository would need to push there is a stored token. So
the tap fetches instead. Its `update.yml`, hourly and by hand (`gh workflow
run update.yml -R localgpt-app/homebrew-tap`), asks npm what `latest` is
and, when the formula is behind, has `cargo xtask formula` at that
release's tag render it, installs it, runs its test and commits it, with
the tap's own `GITHUB_TOKEN`. GitHub stops a schedule in a repository
nothing has touched for 60 days: then run it by hand, which starts it
again.

The cask is the desktop app's, below.

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

## What the tags leave to do by hand

The published `localgpt` points at three things no tag puts up, so they are
done the same day:

- **localgpt.app**, once the crates and the npm package are up: `cargo xtask
  deploy app`. Its docs name the new version's commands, and the command
  line links into them: `localgpt audio` in a build without the models
  answers with `/docs/audio-models/#platforms`.
- **The desktop release and the cask**, once `desktop.yml` has drafted it:
  publish the draft and, if the app is notarized, put its `localgpt.rb` in
  the tap as `Casks/localgpt.rb`. An app that is not notarized stays out of
  the tap, since macOS stops it on first open, and so the README, the
  installation page and `localgpt gen` with no app to hand over to name no
  cask: they send a person to the source build. The first notarized release
  puts `brew install --cask localgpt-app/tap/localgpt` back in all three, in
  a version before it, since the README is the npm package's page as it
  stood when that version was published.
- **The formula**, if the hour is too long to wait: `gh workflow run
  update.yml -R localgpt-app/homebrew-tap`, once the version is on npm.
- **A first version's publisher**: a crate or a package a person uploaded
  for the first time trusts no workflow yet. Give it its trusted publisher
  before the next release ("A crate's first version", "The first version on
  npm").
