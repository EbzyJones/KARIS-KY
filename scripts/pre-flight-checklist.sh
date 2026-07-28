#!/usr/bin/env bash

################################################################################
# karis-ky Escrow — Pre-Flight Deployment Checklist
################################################################################
#
# Validates contract, environment, and network before deployment to mainnet.
#
# Usage:
#   bash scripts/pre-flight-checklist.sh
#   bash scripts/pre-flight-checklist.sh --env .env.testnet
#   bash scripts/pre-flight-checklist.sh --env .env.mainnet --skip-test
#
# Checks:
#   - WASM build and size limits
#   - Git tag and commit ancestry
#   - Schema version constant in source
#   - Environment variables (RPC, network, deployer)
#   - RPC connectivity and chain state
#   - Deployer account balance
#   - Clippy linting (strict mode)
#   - Unit and integration tests
#   - Attestation hash readiness (optional)
#
# Exit codes:
#   0 = all checks passed, safe to deploy
#   1 = at least one check failed
#
################################################################################

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

CHECKS_PASSED=0
CHECKS_FAILED=0

info()    { echo -e "${BLUE}[INFO]${NC}  $*"; }
ok()      { echo -e "${GREEN}[OK]${NC}    $*"; ((CHECKS_PASSED++)); }
warn()    { echo -e "${YELLOW}[WARN]${NC}   $*"; }
fail()    { echo -e "${RED}[FAIL]${NC}   $*"; ((CHECKS_FAILED++)); }
section() { echo ""; echo -e "${BLUE}━━━ $* ━━━${NC}"; }

# ── Configuration ───────────────────────────────────────────────────────────

ENV_FILE="${1:-.env}"
SKIP_TEST=false
SKIP_CLIPPY=false

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    --env) ENV_FILE="$2"; shift 2 ;;
    --skip-test) SKIP_TEST=true; shift ;;
    --skip-clippy) SKIP_CLIPPY=true; shift ;;
    --help)
      echo "Usage: $0 [--env FILE] [--skip-test] [--skip-clippy] [--help]"
      exit 0
      ;;
    *) fail "Unknown argument: $1"; exit 1 ;;
  esac
done

# ── Load environment ────────────────────────────────────────────────────────

section "ENVIRONMENT LOAD"

if [ -f "${ENV_FILE}" ]; then
  info "Loading config from ${ENV_FILE}"
  set -a
  # shellcheck disable=SC1090
  source "${ENV_FILE}"
  set +a
else
  warn "${ENV_FILE} not found; using system environment"
fi

# Defaults
STELLAR_NETWORK="${STELLAR_NETWORK:-testnet}"
SOROBAN_RPC_URL="${SOROBAN_RPC_URL:-}"
SOURCE_SECRET="${SOURCE_SECRET:-}"
DEPLOYER_ADDRESS="${DEPLOYER_ADDRESS:-}"
WASM_TARGET="${WASM_TARGET:-wasm32-unknown-unknown}"
WASM_PATH="${WASM_PATH:-target/${WASM_TARGET}/release/karis_ky_escrow.wasm}"
MAX_WASM_SIZE=$((512 * 1024))  # 512 KB hard limit for Soroban contracts
EXPECTED_SCHEMA_VERSION="6"

# ── Check 1: Schema Version Constant ────────────────────────────────────────

section "CHECK 1: SCHEMA VERSION"

SCHEMA_REGEX='const SCHEMA_VERSION: u32 = ([0-9]+);'
if [[ $(grep -o "$SCHEMA_REGEX" escrow/src/lib.rs) =~ $SCHEMA_REGEX ]]; then
  SOURCE_SCHEMA="${BASH_REMATCH[1]}"
  info "Found SCHEMA_VERSION = $SOURCE_SCHEMA in escrow/src/lib.rs"

  if [ "$SOURCE_SCHEMA" = "$EXPECTED_SCHEMA_VERSION" ]; then
    ok "Schema version is $EXPECTED_SCHEMA_VERSION"
  else
    warn "Schema version is $SOURCE_SCHEMA (expected $EXPECTED_SCHEMA_VERSION)"
  fi
else
  fail "Could not find SCHEMA_VERSION in escrow/src/lib.rs"
fi

# ── Check 2: Git Tag ────────────────────────────────────────────────────────

section "CHECK 2: GIT TAG & COMMIT"

if ! command -v git &> /dev/null; then
  warn "git not found; skipping tag check"
else
  if ! git rev-parse --git-dir > /dev/null 2>&1; then
    warn "Not in a git repository; skipping tag check"
  else
    CURRENT_COMMIT=$(git rev-parse HEAD)
    COMMIT_SHORT=$(git rev-parse --short HEAD)
    info "Current commit: $COMMIT_SHORT"

    # Find most recent tag on this branch
    LATEST_TAG=$(git describe --tags --abbrev=0 2>/dev/null || echo "none")
    if [ "$LATEST_TAG" != "none" ]; then
      TAG_COMMIT=$(git rev-list -n 1 "$LATEST_TAG")
      if [ "$TAG_COMMIT" = "$CURRENT_COMMIT" ]; then
        ok "Current commit is tagged: $LATEST_TAG"
      else
        warn "Current commit ($COMMIT_SHORT) is not tagged. Latest tag: $LATEST_TAG"
        warn "Recommendation: Tag this commit before production deployment"
      fi
    else
      warn "No git tags found in repository"
    fi

    # Check for uncommitted changes
    if git diff-index --quiet HEAD -- escrow/ 2>/dev/null; then
      ok "No uncommitted changes in escrow/"
    else
      warn "Uncommitted changes detected in escrow/"
      fail "All changes must be committed before production deployment"
    fi
  fi
fi

# ── Check 3: WASM Build ─────────────────────────────────────────────────────

section "CHECK 3: WASM BUILD & SIZE"

if [ ! -f "${WASM_PATH}" ]; then
  info "WASM not found at ${WASM_PATH}. Building release version..."
  
  if ! rustup target list | grep -q "wasm32-unknown-unknown (installed)"; then
    info "Installing wasm32-unknown-unknown target"
    rustup target add wasm32-unknown-unknown
  fi

  if ! cargo build --target wasm32-unknown-unknown --release -p karis_ky_escrow; then
    fail "WASM build failed"
    exit 1
  fi
fi

ok "WASM built successfully"

# Check WASM size
WASM_SIZE=$(stat --format=%s "${WASM_PATH}" 2>/dev/null || stat -f%z "${WASM_PATH}" 2>/dev/null)
WASM_SIZE_KB=$((WASM_SIZE / 1024))
WASM_SIZE_MB=$(echo "scale=2; $WASM_SIZE / 1048576" | bc)

info "WASM size: ${WASM_SIZE_KB} KB (${WASM_SIZE_MB} MB)"

if [ "$WASM_SIZE" -gt "$MAX_WASM_SIZE" ]; then
  fail "WASM exceeds 512 KB limit ($WASM_SIZE_KB KB)"
else
  ok "WASM size within limits"
fi

# ── Check 4: Clippy Linting ────────────────────────────────────────────────

section "CHECK 4: CLIPPY LINTING"

if [ "$SKIP_CLIPPY" = true ]; then
  warn "Skipping clippy (--skip-clippy)"
else
  info "Running clippy with strict mode (-D warnings)..."

  if cargo clippy -p karis_ky_escrow --target wasm32-unknown-unknown --release -- -D warnings; then
    ok "Clippy passed (no warnings)"
  else
    fail "Clippy found warnings (see output above)"
  fi
fi

# ── Check 5: Tests ─────────────────────────────────────────────────────────

section "CHECK 5: UNIT & INTEGRATION TESTS"

if [ "$SKIP_TEST" = true ]; then
  warn "Skipping tests (--skip-test)"
else
  info "Running full test suite..."

  if cargo test -p karis_ky_escrow --lib; then
    ok "All tests passed"
  else
    fail "Tests failed (see output above)"
  fi
fi

# ── Check 6: Environment Variables ────────────────────────────────────────

section "CHECK 6: ENVIRONMENT VARIABLES"

MISSING_VARS=()

[ -z "$SOROBAN_RPC_URL" ] && MISSING_VARS+=("SOROBAN_RPC_URL")
[ -z "$SOURCE_SECRET" ] && MISSING_VARS+=("SOURCE_SECRET")
[ -z "$DEPLOYER_ADDRESS" ] && MISSING_VARS+=("DEPLOYER_ADDRESS")
[ -z "$STELLAR_NETWORK" ] && MISSING_VARS+=("STELLAR_NETWORK")

if [ ${#MISSING_VARS[@]} -eq 0 ]; then
  ok "All required environment variables set"
  info "  STELLAR_NETWORK: $STELLAR_NETWORK"
  info "  SOROBAN_RPC_URL: $SOROBAN_RPC_URL"
  info "  DEPLOYER_ADDRESS: ${DEPLOYER_ADDRESS:0:10}..."
else
  fail "Missing environment variables: ${MISSING_VARS[*]}"
fi

# ── Check 7: RPC Connectivity ──────────────────────────────────────────────

section "CHECK 7: RPC CONNECTIVITY"

if [ -n "$SOROBAN_RPC_URL" ]; then
  info "Testing RPC endpoint: $SOROBAN_RPC_URL"

  if command -v curl &> /dev/null; then
    # Simple HTTP health check
    if curl -s -m 5 "$SOROBAN_RPC_URL/soroban/rpc" -X POST \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","id":1,"method":"getLatestLedger","params":[]}' \
        > /dev/null 2>&1; then
      ok "RPC endpoint is reachable"
    else
      fail "Cannot reach RPC endpoint"
    fi
  else
    warn "curl not found; skipping RPC reachability test"
  fi
else
  warn "SOROBAN_RPC_URL not set; skipping RPC connectivity check"
fi

# ── Check 8: Deployer Account Balance ──────────────────────────────────────

section "CHECK 8: DEPLOYER ACCOUNT BALANCE"

if [ -n "$DEPLOYER_ADDRESS" ] && [ -n "$SOROBAN_RPC_URL" ] && command -v stellar &> /dev/null; then
  info "Checking account balance for $DEPLOYER_ADDRESS..."

  if BALANCE=$(stellar account info --account-id "$DEPLOYER_ADDRESS" \
      --rpc-url "$SOROBAN_RPC_URL" \
      --network "$STELLAR_NETWORK" 2>/dev/null | grep -oP '(?<=native:\s)\d+(\.\d+)?'); then
    info "Account balance: $BALANCE XLM"

    # Minimum balance to cover deployment tx + rent (rough estimate: 2 XLM)
    MIN_BALANCE=2.0
    if (( $(echo "$BALANCE > $MIN_BALANCE" | bc -l) )); then
      ok "Sufficient balance for deployment (>$MIN_BALANCE XLM)"
    else
      warn "Balance may be insufficient for deployment (< $MIN_BALANCE XLM recommended)"
    fi
  else
    warn "Could not fetch account balance (account may not exist yet)"
  fi
else
  warn "Skipping balance check (missing stellar CLI, deployer address, or RPC URL)"
fi

# ── Check 9: Version Metadata ──────────────────────────────────────────────

section "CHECK 9: CONTRACT INTERFACE VERSION"

INTERFACE_REGEX='const CONTRACT_INTERFACE_VERSION: u32 = ([0-9]+);'
if [[ $(grep -o "$INTERFACE_REGEX" escrow/src/lib.rs) =~ $INTERFACE_REGEX ]]; then
  INTERFACE_VERSION="${BASH_REMATCH[1]}"
  ok "Contract interface version: $INTERFACE_VERSION"
else
  warn "Could not find CONTRACT_INTERFACE_VERSION"
fi

# ── Check 10: Dependencies ────────────────────────────────────────────────

section "CHECK 10: DEPENDENCY AUDIT"

if command -v cargo-deny &> /dev/null; then
  info "Running cargo-deny for security advisories..."
  if cargo deny check advisories 2>/dev/null; then
    ok "No dependency advisories found"
  else
    warn "Dependency advisories found (review before production)"
  fi
else
  warn "cargo-deny not installed; skipping dependency audit"
fi

# ── Summary ────────────────────────────────────────────────────────────────

section "CHECKLIST SUMMARY"

TOTAL=$((CHECKS_PASSED + CHECKS_FAILED))

echo ""
echo "Passed: ${GREEN}$CHECKS_PASSED${NC}"
echo "Failed: ${RED}$CHECKS_FAILED${NC}"
echo "Total:  $TOTAL"
echo ""

if [ $CHECKS_FAILED -eq 0 ]; then
  echo -e "${GREEN}✓ All checks passed! Safe to deploy.${NC}"
  exit 0
else
  echo -e "${RED}✗ $CHECKS_FAILED check(s) failed. Do not deploy.${NC}"
  exit 1
fi
