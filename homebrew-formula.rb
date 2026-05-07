class Sootie < Formula
  desc "Cross-platform computer-use for AI agents via MCP"
  homepage "https://github.com/joe223/sootie"
  version "0.1.0"
  license "Apache-2.0"
  head "https://github.com/joe223/sootie.git", branch: "main"

  # Pre-compiled binaries (bottles) - update these when releasing
  if OS.mac?
    if Hardware::CPU.intel?
      url "https://github.com/joe223/sootie/releases/download/v#{version}/sootie-macos-x64"
      sha256 :no_check # Replace with actual: shasum -a 256 sootie-macos-x64
    else
      url "https://github.com/joe223/sootie/releases/download/v#{version}/sootie-macos-arm64"
      sha256 :no_check # Replace with actual: shasum -a 256 sootie-macos-arm64
    end
  elsif OS.linux?
    url "https://github.com/joe223/sootie/releases/download/v#{version}/sootie-linux-x64"
    sha256 :no_check # Replace with actual: shasum -a 256 sootie-linux-x64
  end

  # Build dependencies (only needed for --HEAD or if bottle unavailable)
  depends_on "rust" => :build if build.head?

  def install
    binary_name = if OS.mac?
                    if Hardware::CPU.intel?
                      "sootie-macos-x64"
                    else
                      "sootie-macos-arm64"
                    end
                  else
                    "sootie-linux-x64"
                  end

    if build.head?
      # Build from source for --HEAD installs
      system "cargo", "build", "--release", "--locked"
      bin.install "target/release/sootie"
    else
      # Install pre-compiled binary
      bin.install binary_name => "sootie"
    end

    chmod 0755, bin/"sootie"
  end

  def caveats
    <<~EOS
      ✓ Sootie installed successfully!

      Next steps:
        1. Run: sootie setup
        2. Configure your MCP client (Claude Code, Cursor, etc.)

      Vision setup (optional):
        The setup command will prompt to download the vision sidecar model (~2GB).
        This enables visual fallback when accessibility APIs fail.

      Configuration file:
        ~/.config/sootie/config.toml

      Logs:
        ~/Library/Application Support/sootie/logs/sootie.log (macOS)
        ~/.local/share/sootie/logs/sootie.log (Linux)
    EOS
  end

  test do
    assert_match "sootie", shell_output("#{bin}/sootie --version")
  end
end