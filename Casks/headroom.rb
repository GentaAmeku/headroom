# Homebrew Cask for Headroom (unsigned).
#
# This file is a template. To publish a tap:
#   1. Create a public repo named `homebrew-tap` (e.g. github.com/GentaAmeku/homebrew-tap).
#   2. Copy this file to `Casks/headroom.rb` in that repo.
#   3. After each GitHub release, bump `version` and set `sha256` to the .dmg checksum:
#        shasum -a 256 Headroom_<version>_universal.dmg
#      (or keep `sha256 :no_check` for a personal tap).
#
# Users then install with:
#   brew install --cask --no-quarantine GentaAmeku/tap/headroom
cask "headroom" do
  version "0.1.0"
  sha256 :no_check

  url "https://github.com/GentaAmeku/headroom/releases/download/v#{version}/Headroom_#{version}_universal.dmg"
  name "Headroom"
  desc "Menu bar app showing AI coding tool usage at a glance"
  homepage "https://github.com/GentaAmeku/headroom"

  depends_on macos: ">= :big_sur"

  app "Headroom.app"

  zap trash: [
    "~/.config/headroom",
    "~/Library/Application Support/com.gameku.headroom",
    "~/Library/Caches/com.gameku.headroom",
  ]
end
