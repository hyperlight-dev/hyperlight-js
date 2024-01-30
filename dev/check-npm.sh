#!/usr/bin/env bash
set -euo pipefail

if command -v npm &> /dev/null; then
    echo "✓ npm is installed: $(npm --version)"
    exit 0
fi

echo "❌ npm is not installed!"
echo ""

# Determine the install command based on Linux distro
INSTALL_CMD=""
if command -v apt-get &> /dev/null; then
    INSTALL_CMD="sudo apt-get update && sudo apt-get install -y nodejs npm"
    DISTRO="Ubuntu/Debian"
elif command -v dnf &> /dev/null; then
    INSTALL_CMD="sudo dnf install -y nodejs npm"
    DISTRO="Azure Linux/Fedora"
fi

if [[ -z "$INSTALL_CMD" ]]; then
    echo "❌ Unable to auto-install npm on this Linux distribution."
    echo "Please install Node.js and npm from: https://nodejs.org/"
    exit 1
fi

echo "Detected: $DISTRO"
echo "Install command: $INSTALL_CMD"
echo ""

# Check if we're in an interactive terminal
if [[ -t 0 ]]; then
    # Interactive - ask for confirmation
    read -p "Install npm automatically? [y/N] " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "❌ Installation cancelled. Please install npm manually:"
        echo "   $INSTALL_CMD"
        exit 1
    fi
else
    # Non-interactive (CI/automation) - install automatically
    echo "🤖 Non-interactive terminal detected - installing automatically..."
fi

echo "📦 Installing npm..."

# Retry logic for dpkg lock errors
MAX_RETRIES=5
RETRY_DELAY=60
attempt=1

while [ $attempt -le $MAX_RETRIES ]; do
    if [ $attempt -gt 1 ]; then
        echo "⏳ Attempt $attempt of $MAX_RETRIES (waiting ${RETRY_DELAY}s for dpkg lock to be released)..."
        sleep $RETRY_DELAY
    fi
    
    if eval "$INSTALL_CMD" 2>&1 | tee /tmp/npm_install.log; then
        echo ""
        echo "✅ npm installed successfully: $(npm --version)"
        rm -f /tmp/npm_install.log
        exit 0
    else
        # Check if the error is related to dpkg lock
        if grep -qE "Could not get lock|Unable to acquire the dpkg frontend lock|dpkg frontend lock" /tmp/npm_install.log; then
            if [ $attempt -lt $MAX_RETRIES ]; then
                echo "⚠️  dpkg lock is held by another process, retrying..."
            else
                echo ""
                echo "❌ Installation failed after $MAX_RETRIES attempts."
                echo "Another process is holding the dpkg lock. Please wait and try again, or install manually:"
                echo "   $INSTALL_CMD"
                rm -f /tmp/npm_install.log
                exit 1
            fi
        else
            # Different error, don't retry
            echo ""
            echo "❌ Installation failed. Please install manually:"
            echo "   $INSTALL_CMD"
            rm -f /tmp/npm_install.log
            exit 1
        fi
    fi
    
    ((attempt++))
done

# Should not reach here, but just in case
echo "❌ Installation failed after $MAX_RETRIES attempts."
rm -f /tmp/npm_install.log
exit 1
