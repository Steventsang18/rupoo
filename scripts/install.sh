#!/bin/bash
# Yupoo - Linux installation script
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
