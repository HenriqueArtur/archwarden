# Homebrew formula for archwarden.
#
# Pours a bottle rather than building from source: the point of this route is
# installing without a Rust toolchain, and a formula that compiled would need
# the very thing it exists to avoid.
#
# `sha256` and `version` are rewritten by the release workflow. A formula whose
# checksums are edited by hand is a formula that eventually ships the wrong
# ones.
class Archwarden < Formula
  desc "Fast, declarative architecture linter for TypeScript and JavaScript"
  homepage "https://github.com/HenriqueArtur/archwarden"
  version "0.1.0"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/HenriqueArtur/archwarden/releases/download/v#{version}/archwarden-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/HenriqueArtur/archwarden/releases/download/v#{version}/archwarden-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/HenriqueArtur/archwarden/releases/download/v#{version}/archwarden-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/HenriqueArtur/archwarden/releases/download/v#{version}/archwarden-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "archwarden"
  end

  test do
    # Two assertions, because a binary that prints its version can still be
    # unable to do its job. The second exercises config discovery, parsing and
    # the walk -- everything between the user and an answer.
    assert_match version.to_s, shell_output("#{bin}/archwarden --version")

    (testpath/"arch.config.json").write <<~JSON
      {"version": 0, "rules": []}
    JSON
    assert_match "is valid", shell_output("#{bin}/archwarden config validate")
  end
end
