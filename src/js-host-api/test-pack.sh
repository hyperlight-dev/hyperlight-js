#!/bin/bash
# Validate npm packages by packing to /tmp and installing into a clean project.
# Simulates what a consumer would experience after `npm install @hyperlight/js-host-api`.
#
# Prerequisites:
#   - Native .node binary must exist (via `npm run build`)
#   - Generated bindings (index.js, index.d.ts) must be present
#     (via `npx napi prepublish -t npm` or the CI workflow)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="/tmp/hyperlight-npm-test-pack"
INSTALL_DIR="/tmp/hyperlight-npm-test-install"

# ── Cleanup ──────────────────────────────────────────────────────────
rm -rf "${PACK_DIR}" "${INSTALL_DIR}"
mkdir -p "${PACK_DIR}" "${INSTALL_DIR}"

cd "${SCRIPT_DIR}"

# ── Preflight checks ────────────────────────────────────────────────
if [ ! -f "package.json" ]; then
    echo "❌ Error: package.json not found. Run from src/js-host-api/" >&2
    exit 1
fi

# In CI the .node binary is already in npm/linux-x64-gnu/; locally it's in the project root.
if ls npm/linux-x64-gnu/*.node 1>/dev/null 2>&1; then
    echo "📦 Platform binary already present in npm/linux-x64-gnu/"
elif ls ./*.node 1>/dev/null 2>&1; then
    NATIVE_BINARY=$(ls ./*.node | head -1)
    BINARY_NAME=$(basename "${NATIVE_BINARY}")
    echo "📦 Copying ${BINARY_NAME} into platform package..."
    cp "${NATIVE_BINARY}" npm/linux-x64-gnu/"${BINARY_NAME}"
else
    echo "❌ Error: No .node binary found. Run 'npm run build' first, or ensure CI artifacts are staged." >&2
    exit 1
fi

# ── Step 1: Pack platform package ───────────────────────────────────
echo "📦 Packing platform package (linux-x64-gnu)..."
PLATFORM_TGZ=$(npm pack ./npm/linux-x64-gnu --pack-destination "${PACK_DIR}" 2>/dev/null)
PLATFORM_TGZ_PATH="${PACK_DIR}/${PLATFORM_TGZ}"
echo "   → ${PLATFORM_TGZ_PATH}"

# ── Step 2: Pack main package ───────────────────────────────────────
echo "📦 Packing main package..."
MAIN_TGZ=$(npm pack --pack-destination "${PACK_DIR}" 2>/dev/null)
MAIN_TGZ_PATH="${PACK_DIR}/${MAIN_TGZ}"
echo "   → ${MAIN_TGZ_PATH}"

# ── Step 3: Inspect tarball contents ────────────────────────────────
echo ""
echo "🔍 Platform package contents:"
tar tzf "${PLATFORM_TGZ_PATH}" | sed 's/^/   /'

echo ""
echo "🔍 Main package contents:"
tar tzf "${MAIN_TGZ_PATH}" | sed 's/^/   /'

# ── Step 4: Validate main package contents ──────────────────────────
echo ""
echo "✅ Validating main package contents..."
MAIN_FILES=$(tar tzf "${MAIN_TGZ_PATH}")

REQUIRED_FILES=("package/package.json" "package/lib.js" "package/index.js" "package/index.d.ts" "package/lib.d.ts")
for f in "${REQUIRED_FILES[@]}"; do
    if echo "${MAIN_FILES}" | grep -q "^${f}$"; then
        echo "   ✅ ${f}"
    else
        echo "   ❌ MISSING: ${f}" >&2
        exit 1
    fi
done

BANNED_PATTERNS=("package/src/" "package/tests/" "package/Cargo.toml" "package/node_modules/" "package/target/")
for p in "${BANNED_PATTERNS[@]}"; do
    if echo "${MAIN_FILES}" | grep -q "^${p}"; then
        echo "   ❌ LEAKED: ${p}" >&2
        exit 1
    else
        echo "   ✅ No leak: ${p}"
    fi
done

if echo "${MAIN_FILES}" | grep -q '\.node$'; then
    echo "   ❌ LEAKED: .node binary in main package (should only be in platform packages)" >&2
    exit 1
else
    echo "   ✅ No leak: *.node"
fi

# ── Step 5: Validate platform package contents ──────────────────────
echo ""
echo "✅ Validating platform package contents..."
PLATFORM_FILES=$(tar tzf "${PLATFORM_TGZ_PATH}")

if echo "${PLATFORM_FILES}" | grep -q '\.node$'; then
    echo "   ✅ .node binary present"
else
    echo "   ❌ MISSING: .node binary" >&2
    exit 1
fi

# ── Step 6: Install from tarballs into a clean directory ────────────
echo ""
echo "📥 Installing from tarballs into ${INSTALL_DIR}..."
cd "${INSTALL_DIR}"
npm init -y --silent >/dev/null 2>&1

# Install platform package first, then main package
npm install "${PLATFORM_TGZ_PATH}" --no-save 2>&1 | sed 's/^/   /'
npm install "${MAIN_TGZ_PATH}" --no-save 2>&1 | sed 's/^/   /'

# ── Step 7: Smoke test — require and check exports ──────────────────
echo ""
echo "🧪 Smoke test: require('@hyperlight/js-host-api')..."
EXPORTS=$(node -e "
    const h = require('@hyperlight/js-host-api');
    const keys = Object.keys(h);
    if (keys.length === 0) {
        console.error('ERROR: No exports found');
        process.exit(1);
    }
    console.log('Exports:', keys.join(', '));
")
echo "   ${EXPORTS}"

# ── Step 8: Hello World — end-to-end sandbox test ───────────────────
echo ""
echo "🧪 Hello World: create sandbox, load handler, call it..."
node -e "
    const { SandboxBuilder } = require('@hyperlight/js-host-api');

    async function main() {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const jsSandbox = await proto.loadRuntime();

        jsSandbox.addHandler(
            'hello',
            'function handler(event) { event.greeting = \"Hello from Hyperlight!\"; return event; }'
        );

        const loaded = await jsSandbox.getLoadedSandbox();
        const result = await loaded.callHandler('hello', {}, { gc: false });

        if (result.greeting !== 'Hello from Hyperlight!') {
            console.error('ERROR: unexpected result:', JSON.stringify(result));
            process.exit(1);
        }
        console.log('   ✅ Got:', result.greeting);
    }

    main().catch(err => { console.error('   ❌', err.message); process.exit(1); });
"

# ── Done ────────────────────────────────────────────────────────────
echo ""
echo "🎉 All checks passed! Package is ready to ship."
