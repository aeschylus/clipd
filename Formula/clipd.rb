class Clipd < Formula
  desc "Clipboard history manager with global hotkey and floating panel UI"
  homepage "https://github.com/aeschylus/clipd"
  # Update the URL and sha256 when a release is tagged on GitHub.
  # Run: brew fetch --build-from-source Formula/clipd.rb
  url "https://github.com/aeschylus/clipd/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "" # fill in after tagging a release: shasum -a 256 clipd-0.2.0.tar.gz
  license "MIT"
  head "https://github.com/aeschylus/clipd.git", branch: "main"

  depends_on :macos
  depends_on "rust" => :build

  def install
    # Build the CLI tool only (the .app bundle requires Tauri toolchain)
    system "cargo", "build", "--release", "--package", "clipd",
           *std_configure_args
    bin.install "target/release/clipd"
  end

  # Install the macOS .app bundle from a pre-built GitHub release asset.
  # Uncomment and update the url/sha256 once a release DMG is available.
  #
  # artifact "clipd.app", target: "#{prefix}/clipd.app"
  # app "clipd.app"

  service do
    run [opt_bin/"clipd", "daemon", "start", "--foreground"]
    keep_alive true
    log_path var/"log/clipd.log"
    error_log_path var/"log/clipd.log"
  end

  def caveats
    <<~EOS
      clipd has been installed as a command-line tool.

      Start the clipboard daemon:
        clipd daemon start

      For the full macOS app with menu bar and global hotkey (Cmd+Shift+V),
      download clipd.app from:
        https://github.com/aeschylus/clipd/releases

      Or build it yourself:
        git clone https://github.com/aeschylus/clipd
        cd clipd
        # Install Tauri CLI: cargo install tauri-cli
        cargo tauri build

      Grant Accessibility permissions for source-app detection:
        System Settings -> Privacy & Security -> Accessibility -> clipd
    EOS
  end

  test do
    # Verify the CLI binary works
    assert_match "clipd", shell_output("#{bin}/clipd --version")
    # Check that config command runs without error
    system bin/"clipd", "config"
  end
end
