#!/bin/sh
# Rupoo - cross-platform installer (macOS / Linux)
#
# Downloads the official release binary from GitHub Releases,
# verifies its SHA-256 checksum, and installs it to ~/.local/bin.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Steventsang18/rupoo/master/scripts/install.sh | sh
#   sh scripts/install.sh                # latest version
#   sh scripts/install.sh -v 0.6.3       # pinned version
#   sh scripts/install.sh -d /opt/bin    # custom install dir
#
# Requires: curl or wget, tar, shasum (macOS) / sha256sum (Linux).

set -eu

REPO="Steventsang18/rupoo"
VERSION=""
INSTALL_DIR="${RUPOO_INSTALL_DIR:-${HOME}/.local/bin}"
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t rupoo)"

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

# --- Argument parsing ---
while [ "$#" -gt 0 ]; do
    case "$1" in
        -v|--version) VERSION="${2:-}"; shift 2 ;;
        -d|--dir)     INSTALL_DIR="${2:-}"; shift 2 ;;
        -h|--help)
            echo "Usage: install.sh [-v VERSION] [-d DIR]"
            echo "  -v VERSION   Release version (default: latest)"
            echo "  -d DIR       Install directory (default: ~/.local/bin)"
            exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

# --- Detect platform + architecture ---
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
    darwin) PLATFORM="apple-darwin" ;;
    linux)  PLATFORM="unknown-linux-gnu" ;;
    *) echo "Unsupported OS: $OS (only macOS and Linux are supported)" >&2; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64) TARGET="x86_64-$PLATFORM" ;;
    aarch64|arm64) TARGET="aarch64-$PLATFORM" ;;
    *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

# --- Resolve version ---
if [ -z "$VERSION" ]; then
    echo "> Resolving latest release version..."
    VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' | head -n1)"
    if [ -z "$VERSION" ]; then
        echo "Error: could not resolve the latest release version." >&2
        exit 1
    fi
fi
echo "> Installing rupoo v${VERSION} (${TARGET})"

ARCHIVE="rupoo-v${VERSION}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}"

# --- Download binary + checksum ---
echo "> Downloading ${ARCHIVE}..."
if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$TMP_DIR/rupoo.tar.gz" "$BASE_URL/$ARCHIVE" \
        || { echo "Download failed. Check version/platform." >&2; exit 1; }
    curl -fsSL -o "$TMP_DIR/rupoo.sha256" "$BASE_URL/$ARCHIVE.sha256" \
        || { echo "Warning: checksum file not found; skipping verification." >&2; }
else
    wget -q -O "$TMP_DIR/rupoo.tar.gz" "$BASE_URL/$ARCHIVE" \
        || { echo "Download failed. Check version/platform." >&2; exit 1; }
    wget -q -O "$TMP_DIR/rupoo.sha256" "$BASE_URL/$ARCHIVE.sha256" \
        || { echo "Warning: checksum file not found; skipping verification." >&2; }
fi

# --- Verify SHA-256 ---
if [ -f "$TMP_DIR/rupoo.sha256" ]; then
    echo "> Verifying SHA-256 checksum..."
    EXPECTED="$(awk '{print $1}' "$TMP_DIR/rupoo.sha256")"
    if command -v shasum >/dev/null 2>&1; then
        ACTUAL="$(shasum -a 256 "$TMP_DIR/rupoo.tar.gz" | awk '{print $1}')"
    elif command -v sha256sum >/dev/null 2>&1; then
        ACTUAL="$(sha256sum "$TMP_DIR/rupoo.tar.gz" | awk '{print $1}')"
    else
        echo "Warning: no sha256 tool found; skipping verification." >&2
        ACTUAL="$EXPECTED"
    fi
    if [ "$ACTUAL" != "$EXPECTED" ]; then
        echo "Error: checksum mismatch!" >&2
        echo "  expected: $EXPECTED" >&2
        echo "  actual:   $ACTUAL" >&2
        exit 1
    fi
    echo "> Checksum OK."
fi

# --- Extract + install ---
echo "> Extracting..."
tar xzf "$TMP_DIR/rupoo.tar.gz" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$TMP_DIR/rupoo" "$INSTALL_DIR/rupoo"

# --- Verify + PATH hint ---
if [ -x "$INSTALL_DIR/rupoo" ]; then
    echo ""
    echo "✓ rupoo v${VERSION} installed to $INSTALL_DIR/rupoo"
    "$INSTALL_DIR/rupoo" --version
else
    echo "Error: installation failed." >&2
    exit 1
fi

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        echo "Add $INSTALL_DIR to your PATH:"
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        echo "  (add the line above to ~/.zshrc or ~/.bashrc)"
        ;;
esac

echo ""
echo "Run 'rupoo' to start the interactive REPL."
#!/bin/bash
# Rupoo - Linux installation script
# Copies the compiled binary to /usr/local/bin

set -e

BINARY="./target/release/rupoo"
INSTALL_DIR="/usr/local/bin"

if [ ! -f "$BINARY" ]; then
    echo "Error: Binary not found at $BINARY"
    echo "Please run 'cargo build --release' first."
    exit 1
fi

echo "Installing rupoo to $INSTALL_DIR..."
sudo cp "$BINARY" "$INSTALL_DIR/rupoo"
sudo chmod +x "$INSTALL_DIR/rupoo"

echo "Verifying installation..."
if command -v rupoo &> /dev/null; then
    echo "✓ rupoo installed successfully: $(which rupoo)"
    rupoo --version
else
    echo "Error: Installation failed."
    exit 1
fi

echo ""
echo "Run 'rupoo' to start the interactive REPL."
