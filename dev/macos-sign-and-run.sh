#!/usr/bin/env bash
# Cargo target runner for macOS.
#
# Hypervisor.framework refuses hv_vm_create() with HV_DENIED (0xfae94007)
# unless the calling process carries the com.apple.security.hypervisor
# entitlement, so ad-hoc sign each test/bench/example binary before running
# it. Mirrors the same script in hyperlight-dev/hyperlight.
set -Eeuo pipefail

codesign -f -s - --entitlements "$(dirname "$0")/macos-entitlements.plist" "$1"
exec "$@"
