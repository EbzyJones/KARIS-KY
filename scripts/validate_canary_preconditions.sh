#!/usr/bin/env bash
#
# Validate canary deployment preconditions before a staged rollout.
#
# This script checks that:
# 1. WASM has been built and exists in the expected location
# 2. All CI gates pass (format, lint, tests, coverage)
# 3. Build metadata matches source constants (schema version, interface version)
# 4. Git commit hash is resolvable
#
# Exit codes:
#   0 — All preconditions met; canary deployment may proceed
#   1 — One or more preconditions failed; see output for details
#   2 — Usage error or script misconfiguration
#
# Usage:
#   bash scripts/validate_canary_preconditions.sh
#   bash scripts/validate_canary_preconditions.sh --wasm-dir target/wasm32v1-none/release

set -euo pipefail

# --- Configuration ---
WASM_DIR="${1:---wasm-dir}"
if [[ "$WASM_DIR" == "--wasm-dir" ]]; then
    shift || true
    WASM_DIR="${1:-target/wasm32v1-none/release}"
fi

WASM_FILE="${WASM_DIR}/karis_ky_escrow.wasm"
BUILD_METADATA="${WASM_DIR}/../../../target/build_metadata.json"
BUILD_METADATA_FALLBACK="target/build_metadata.json"
LIB_RS="escrow/src/lib.rs"

# --- Color output ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# --- Utility functions ---
log_info() {
    echo -e "${GREEN}✓${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}⚠${NC} $*" >&2
}

log_error() {
    echo -e "${RED}✗${NC} $*" >&2
}

# --- Precondition checks ---

echo "─────────────────────────────────────────────────────────────"
echo "Canary Deployment Precondition Validator"
echo "─────────────────────────────────────────────────────────────"

failed=0
passed=0

# Check 1: WASM file exists
echo ""
echo "Check 1: WASM built"
if [[ -f "$WASM_FILE" ]]; then
    log_info "WASM exists: $WASM_FILE"
    ((passed++))
else
    log_error "WASM not found: $WASM_FILE"
    log_error "  Run: cargo build --target wasm32v1-none --release"
    ((failed++))
fi

# Check 2: Build metadata exists
echo ""
echo "Check 2: Build metadata recorded"
if [[ -f "$BUILD_METADATA" ]]; then
    BUILD_METADATA_PATH="$BUILD_METADATA"
elif [[ -f "$BUILD_METADATA_FALLBACK" ]]; then
    BUILD_METADATA_PATH="$BUILD_METADATA_FALLBACK"
else
    log_error "Build metadata not found"
    log_error "  Expected: $BUILD_METADATA or $BUILD_METADATA_FALLBACK"
    log_error "  Run: cargo build to generate build_metadata.json"
    ((failed++))
fi

# Check 3: Format check
echo ""
echo "Check 3: Code format (cargo fmt)"
if cargo fmt --all -- --check >/dev/null 2>&1; then
    log_info "Code format is valid"
    ((passed++))
else
    log_error "Code format check failed"
    log_error "  Run: cargo fmt --all"
    ((failed++))
fi

# Check 4: Lint check
echo ""
echo "Check 4: Lint check (cargo clippy)"
if cargo clippy -p karis-ky_escrow -- -D warnings >/dev/null 2>&1; then
    log_info "Lint check passed"
    ((passed++))
else
    log_error "Lint check failed"
    log_error "  Run: cargo clippy -p karis-ky_escrow -- -D warnings"
    ((failed++))
fi

# Check 5: Tests pass
echo ""
echo "Check 5: Unit tests (cargo test)"
if cargo test -p karis-ky_escrow >/dev/null 2>&1; then
    log_info "All tests passed"
    ((passed++))
else
    log_error "Tests failed"
    log_error "  Run: cargo test -p karis-ky_escrow"
    ((failed++))
fi

# Check 6: Coverage threshold
echo ""
echo "Check 6: Coverage threshold (≥95%)"
if command -v cargo-llvm-cov >/dev/null 2>&1; then
    if cargo llvm-cov --fail-under-lines 95 >/dev/null 2>&1; then
        log_info "Coverage threshold met (≥95%)"
        ((passed++))
    else
        log_error "Coverage below threshold"
        log_error "  Run: cargo llvm-cov --fail-under-lines 95"
        ((failed++))
    fi
else
    log_warn "cargo-llvm-cov not installed; skipping coverage check"
    log_warn "  Install: cargo install cargo-llvm-cov"
fi

# Check 7: Build metadata validation
echo ""
echo "Check 7: Build metadata validation"
if [[ -n "${BUILD_METADATA_PATH:-}" ]] && [[ -f "$BUILD_METADATA_PATH" ]]; then
    if python3 scripts/validate_build_metadata.py --json "$BUILD_METADATA_PATH" --lib "$LIB_RS" >/dev/null 2>&1; then
        log_info "Build metadata is valid"
        ((passed++))
    else
        log_error "Build metadata validation failed"
        log_error "  Run: python3 scripts/validate_build_metadata.py"
        ((failed++))
    fi
else
    log_warn "Build metadata not available; skipping validation"
fi

# Check 8: Git commit resolvable
echo ""
echo "Check 8: Git commit hash"
if commit=$(git rev-parse HEAD 2>/dev/null); then
    short_commit=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
    log_info "Git commit: $short_commit ($commit)"
    ((passed++))
else
    log_error "Git commit not resolvable"
    log_error "  Are you in a git repository?"
    ((failed++))
fi

# --- Summary ---
echo ""
echo "─────────────────────────────────────────────────────────────"
echo "Summary: $passed passed, $failed failed"
echo "─────────────────────────────────────────────────────────────"

if (( failed > 0 )); then
    echo ""
    log_error "Canary preconditions NOT met. Fix errors above before deploying."
    exit 1
fi

echo ""
log_info "All preconditions met! Canary deployment may proceed."
echo ""
echo "Next steps:"
echo "  1. Print docs/CANARY_DEPLOYMENT_CHECKLIST.md"
echo "  2. Follow Phase 2 (Canary Deployment) section"
echo "  3. Record WASM hash: $commit"
echo ""
exit 0
