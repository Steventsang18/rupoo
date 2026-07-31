#!/bin/bash
# Yupoo - AI Plan Executor Agent
# Double-click this file to open a terminal and enter the interactive REPL.

# Resolve the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# Assume the rupoo binary is installed in PATH
YUPOO_BIN="$(which rupoo 2>/dev/null || echo "$SCRIPT_DIR/../target/release/rupoo")"

if [ ! -x "$YUPOO_BIN" ]; then
    echo "Error: rupoo binary not found at '$YUPOO_BIN'"
    echo "Please run 'cargo install --path .' from the project directory first."
    read -p "Press Enter to close this window..."
    exit 1
fi

clear
"$YUPOO_BIN"

echo ""
read -p "Press Enter to close this window..."
