#!/bin/bash
# Validate npm packages by packing to /tmp and installing into a clean project.
# Simulates what a consumer would experience after `npm install @hyperlight/js-host-api`.
#
# Prerequisites: run `npm run build` first to produce the .node binary.

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

if ! ls ./*.node 1>/dev/null 2>&1; then
    echo "❌ Error: No .node binary found. Run 'npm run build' first." >&2
    exit 1
fi

# ── Step 1: Copy the .node binary into the platform package ─────────
echo "📦 Preparing platform package..."
NATIVE_BINARY=$(ls ./*.node | head -1)
BINARY_NAME=$(basename "${NATIVE_BINARY}")
cp "${NATIVE_BINARY}" npm/linux-x64-gnu/"${BINARY_NAME}"

# ── Step 2: Pack platform package ───────────────────────────────────
echo "📦 Packing platform package (linux-x64-gnu)..."
PLATFORM_TGZ=$(npm pack ./npm/linux-x64-gnu --pack-destination "${PACK_DIR}" 2>/dev/null)
PLATFORM_TGZ_PATH="${PACK_DIR}/${PLATFORM_TGZ}"
echo "   → ${PLATFORM_TGZ_PATH}"

# ── Step 3: Pack main package ───────────────────────────────────────
echo "📦 Packing main package..."
MAIN_TGZ=$(npm pack --pack-destination "${PACK_DIR}" 2>/dev/null)
MAIN_TGZ_PATH="${PACK_DIR}/${MAIN_TGZ}"
echo "   → ${MAIN_TGZ_PATH}"

# ── Step 4: Inspect tarball contents ────────────────────────────────
echo ""
echo "🔍 Platform package contents:"
tar tzf "${PLATFORM_TGZ_PATH}" | sed 's/^/   /'

echo ""
echo "🔍 Main package contents:"
tar tzf "${MAIN_TGZ_PATH}" | sed 's/^/   /'

# ── Step 5: Validate main package contents ──────────────────────────
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

# ── Step 6: Validate platform package contents ──────────────────────
echo ""
echo "✅ Validating platform package contents..."
PLATFORM_FILES=$(tar tzf "${PLATFORM_TGZ_PATH}")

if echo "${PLATFORM_FILES}" | grep -q '\.node$'; then
    echo "   ✅ .node binary present"
else
    echo "   ❌ MISSING: .node binary" >&2
    exit 1
fi

# ── Step 7: Install from tarballs into a clean directory ────────────
echo ""
echo "📥 Installing from tarballs into ${INSTALL_DIR}..."
cd "${INSTALL_DIR}"
npm init -y --silent >/dev/null 2>&1

# Install platform package first, then main package
npm install "${PLATFORM_TGZ_PATH}" --no-save 2>&1 | sed 's/^/   /'
npm install "${MAIN_TGZ_PATH}" --no-save 2>&1 | sed 's/^/   /'

# ── Step 8: Smoke test — require and check exports ──────────────────
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

# ── Step 9: Hello World — end-to-end sandbox test ───────────────────
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

# ── Cleanup temp .node from platform dir ────────────────────────────
rm -f "${SCRIPT_DIR}/npm/linux-x64-gnu/${BINARY_NAME}"

# ── Done ────────────────────────────────────────────────────────────
echo ""
echo "🎉 All checks passed! Package is ready to ship."
