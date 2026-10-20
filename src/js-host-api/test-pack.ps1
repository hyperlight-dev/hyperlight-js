# Validate Windows ARM64 release tarballs on a native ARM64 host with WHP enabled.
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$platform = node -p "process.platform + '-' + process.arch"
if ($LASTEXITCODE -ne 0 -or $platform -ne "win32-arm64") {
    throw "Windows ARM64 package validation requires native ARM64 Node.js on Windows"
}

$packageDir = Join-Path $PSScriptRoot "npm\win32-arm64-msvc"
$binaryName = "js-host-api.win32-arm64-msvc.node"
$binaryPath = Join-Path $packageDir $binaryName
if (-not (Test-Path $binaryPath)) {
    Copy-Item (Join-Path $PSScriptRoot $binaryName) $binaryPath
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("hyperlight-npm-test-" + [guid]::NewGuid())
$null = New-Item -ItemType Directory -Path $tempDir
Push-Location $PSScriptRoot
try {
    $platformPack = npm pack $packageDir --pack-destination $tempDir --json | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "Failed to pack Windows ARM64 package" }
    if ($binaryName -notin $platformPack.files.path) {
        throw "Windows ARM64 package is missing $binaryName"
    }

    $mainPack = npm pack --pack-destination $tempDir --json | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "Failed to pack main package" }
    foreach ($file in @("package.json", "lib.js", "lib.mjs", "index.js", "index.d.ts", "lib.d.ts")) {
        if ($file -notin $mainPack.files.path) {
            throw "Main package is missing $file"
        }
    }
    foreach ($file in $mainPack.files.path) {
        if ($file -match '^(src|tests|node_modules|target)/|^Cargo\.toml$|\.node$') {
            throw "Main package contains unexpected file: $file"
        }
    }

    Set-Location $tempDir
    npm init -y --silent
    if ($LASTEXITCODE -ne 0) { throw "Failed to initialize consumer project" }
    npm install (Join-Path $tempDir $platformPack.filename) (Join-Path $tempDir $mainPack.filename) --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "Failed to install release tarballs" }

    node -e @'
const assert = require('node:assert/strict');
const { SandboxBuilder } = require('@hyperlight-dev/js-host-api');
async function main() {
    assert.equal(typeof SandboxBuilder, 'function');
    const proto = await new SandboxBuilder().build();
    const sandbox = await proto.loadRuntime();
    sandbox.addHandler('hello', 'function handler(event) { event.greeting = "Hello from Hyperlight!"; return event; }');
    const loaded = await sandbox.getLoadedSandbox();
    const result = await loaded.callHandler('hello', {}, { gc: false });
    assert.equal(result.greeting, 'Hello from Hyperlight!');
    console.log('Windows ARM64 release tarballs loaded and executed a WHP sandbox successfully');
}
main().catch(error => { console.error(error); process.exit(1); });
'@
    if ($LASTEXITCODE -ne 0) { throw "Windows ARM64 WHP smoke test failed" }
} finally {
    Pop-Location
    Remove-Item -LiteralPath $tempDir -Recurse -Force
}
