class Cbrlm < Formula
  desc "Codebase RLM Memory MCP — Rust knowledge graph server for AI coding agents"
  homepage "https://github.com/cbrlm/cbrlm"
  license "MIT"
  version "0.1.0"

  on_macos do
    on_arm do
      url "https://github.com/cbrlm/cbrlm/releases/download/v#{version}/cbrlm-macos-arm64.tar.gz"
      sha256 "UPDATE_FROM_RELEASE_SHA256SUMS"
    end
    on_intel do
      url "https://github.com/cbrlm/cbrlm/releases/download/v#{version}/cbrlm-macos-x64.tar.gz"
      sha256 "UPDATE_FROM_RELEASE_SHA256SUMS"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/cbrlm/cbrlm/releases/download/v#{version}/cbrlm-linux-arm64.tar.gz"
      sha256 "UPDATE_FROM_RELEASE_SHA256SUMS"
    end
    on_intel do
      url "https://github.com/cbrlm/cbrlm/releases/download/v#{version}/cbrlm-linux-x64.tar.gz"
      sha256 "UPDATE_FROM_RELEASE_SHA256SUMS"
    end
  end

  def install
    bin.install "cbrlm"
  end

  def post_install
    ohai "Run 'cbrlm install --yes --all' to configure MCP agents"
  end

  livecheck do
    url :stable
    strategy :github_latest
  end

  def caveats
    <<~EOS
      Run `cbrlm install --yes --all` to register the MCP server with coding agents.
      Optional graph UI: `cbrlm --ui --port 9749`
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cbrlm --version")
  end
end