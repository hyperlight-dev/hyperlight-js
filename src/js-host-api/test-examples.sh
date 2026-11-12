#!/bin/bash
# Script to test if the js-host-api examples can run

set -e

exec just --justfile "$(dirname "${BASH_SOURCE[0]}")/../../Justfile" run-js-host-api-examples "$@"
