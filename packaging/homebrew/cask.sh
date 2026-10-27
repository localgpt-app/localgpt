#!/bin/bash
# The Homebrew cask for LocalGPT.app, rendered for one release: the version
# and the two .dmg files go in, the cask text comes out. It belongs in the
# tap (github.com/localgpt-app/homebrew-tap) as Casks/localgpt.rb, once the
# desktop-v<version> release is published, since its URLs are that release's:
#
#   brew install --cask localgpt-app/tap/localgpt
#
# Usage:
#   packaging/homebrew/cask.sh <version> <LocalGPT-<version>-arm64.dmg> <LocalGPT-<version>-x86_64.dmg>
#
# .github/workflows/desktop.yml runs it with the release's files and attaches
# the result to the draft release as localgpt.rb. Bash 3.2 safe.
set -euo pipefail

if [ $# -ne 3 ]; then
  sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
fi
version="$1"
for f in "$2" "$3"; do
  [ -f "$f" ] || { echo "cask.sh: no such file: $f" >&2; exit 2; }
done
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
arm="$(sha "$2")"
intel="$(sha "$3")"

cat <<EOF
# LocalGPT.app for macOS, from the desktop-v${version} GitHub release.
# Rendered by packaging/homebrew/cask.sh in the localgpt repository.
cask "localgpt" do
  arch arm: "arm64", intel: "x86_64"

  version "${version}"
  sha256 arm:   "${arm}",
         intel: "${intel}"

  url "https://github.com/localgpt-app/localgpt/releases/download/desktop-v#{version}/LocalGPT-#{version}-#{arch}.dmg"
  name "LocalGPT"
  desc "Desktop app where a prompt, a document, a song or a screenplay becomes a .world"
  homepage "https://localgpt.app/"

  livecheck do
    url :url
    strategy :github_releases
    regex(/^desktop-v?(\d+(?:\.\d+)+)$/i)
  end

  depends_on macos: ">= :big_sur"

  app "LocalGPT.app"

  zap trash: [
    "~/.local/state/localgpt",
  ]
end
EOF
