class Cbm < Formula
  desc "Codebase Memory MCP — Rust knowledge graph server for AI coding agents"
  homepage "https://github.com/cbm/cbm"
  license "MIT"
  version "0.1.0"

  on_macos do
    on_arm do
      url "https://github.com/cbm/cbm/releases/download/v#{version}/cbm-macos-arm64.tar.gz"
      sha256 "UPDATE_FROM_RELEASE_SHA256SUMS"
    end
    on_intel do
      url "https://github.com/cbm/cbm/releases/download/v#{version}/cbm-macos-x64.tar.gz"
      sha256 "UPDATE_FROM_RELEASE_SHA256SUMS"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/cbm/cbm/releases/download/v#{version}/cbm-linux-arm64.tar.gz"
      sha256 "UPDATE_FROM_RELEASE_SHA256SUMS"
    end
    on_intel do
      url "https://github.com/cbm/cbm/releases/download/v#{version}/cbm-linux-x64.tar.gz"
      sha256 "UPDATE_FROM_RELEASE_SHA256SUMS"
    end
  end

  def install
    bin.install "cbm"
  end

  def post_install
    ohai "Run 'cbm install --yes --all' to configure MCP agents"
  end

  livecheck do
    url :stable
    strategy :github_latest
  end

  def caveats
    <<~EOS
      Run `cbm install --yes --all` to register the MCP server with coding agents.
      Optional graph UI: `cbm --ui --port 9749`
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cbm --version")
  end
end
