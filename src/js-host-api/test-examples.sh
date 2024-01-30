#!/bin/bash
# Script to test if the js-host-api examples can run

set -e

echo "Testing Hyperlight JS Host API Examples"
echo "========================================"
echo ""

# Check if we're in the right directory
if [ ! -f "package.json" ]; then
    echo "Error: Please run this script from the js-host-api directory"
    exit 1
fi

# Check if the native module is built
if [ ! -f "index.js" ]; then
    echo "Native module not found. Building..."
    npm install
    npm run build
    echo ""
fi

echo "Running simple.js example..."
echo "----------------------------"
node examples/simple.js
echo ""

echo "Running calculator.js example..."
echo "---------------------------------"
node examples/calculator.js
echo ""

echo "Running unload.js example..."
echo "----------------------------"
node examples/unload.js
echo ""

echo "Running interrupt.js example..."
echo "--------------------------------"
node examples/interrupt.js
echo ""

echo "Running cpu-timeout.js example..."
echo "----------------------------------"
node examples/cpu-timeout.js
echo ""

echo "========================================"
echo "All examples completed successfully!"
