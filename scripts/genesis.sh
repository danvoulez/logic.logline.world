#!/usr/bin/env bash
set -euo pipefail

# ╔══════════════════════════════════════════════════════════════════════════╗
# ║                                                                        ║
# ║   GENESIS — The First Record of logline.world                          ║
# ║                                                                        ║
# ║   This script is meant to be run exactly once.                         ║
# ║   It creates the world: the founder, the tenant, the first breath.     ║
# ║                                                                        ║
# ╚══════════════════════════════════════════════════════════════════════════╝

FOUNDER_EMAIL="dan@danvoulez.com"
FOUNDER_NAME="Dan Voulez"
TENANT_SLUG="voulezvous"
TENANT_NAME="VoulezVous"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
DIM='\033[2m'
RESET='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
CLI="$REPO_ROOT/target/release/logline-cli"

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${RESET}"
echo -e "${BOLD}║                                                            ║${RESET}"
echo -e "${BOLD}║   ${CYAN}G E N E S I S${RESET}${BOLD}                                          ║${RESET}"
echo -e "${BOLD}║   ${DIM}The First Record of logline.world${RESET}${BOLD}                       ║${RESET}"
echo -e "${BOLD}║                                                            ║${RESET}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${RESET}"
echo ""

# ── Collect system metadata ──────────────────────────────────────────────

GENESIS_TS=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
GENESIS_TS_LOCAL=$(date +"%Y-%m-%d %H:%M:%S %Z")
HOSTNAME=$(hostname)
COMPUTER_NAME=$(scutil --get ComputerName 2>/dev/null || hostname)
OS_NAME=$(sw_vers -productName 2>/dev/null || uname -s)
OS_VERSION=$(sw_vers -productVersion 2>/dev/null || uname -r)
OS_BUILD=$(sw_vers -buildVersion 2>/dev/null || echo "unknown")
ARCH=$(uname -m)
KERNEL=$(uname -r)
SHELL_VERSION=$($SHELL --version 2>&1 | head -1 || echo "$SHELL")
RUST_VERSION=$(rustc --version 2>/dev/null || echo "unknown")
CARGO_VERSION=$(cargo --version 2>/dev/null || echo "unknown")
GIT_HASH=$(cd "$REPO_ROOT" && git rev-parse --short HEAD 2>/dev/null || echo "unknown")
GIT_HASH_FULL=$(cd "$REPO_ROOT" && git rev-parse HEAD 2>/dev/null || echo "unknown")
USERNAME=$(whoami)
USER_FULLNAME=$(id -F 2>/dev/null || echo "$USERNAME")
CHIP=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "unknown")
MEMORY_GB=$(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 ))
SERIAL=$(system_profiler SPHardwareDataType 2>/dev/null | awk '/Serial Number/{print $NF}' || echo "redacted")
UPTIME=$(uptime | sed 's/.*up //' | sed 's/,.*//')

# ── Pre-flight ───────────────────────────────────────────────────────────

echo -e "${DIM}Collecting system information...${RESET}"
echo -e "  Computer:  ${BOLD}${COMPUTER_NAME}${RESET} (${HOSTNAME})"
echo -e "  System:    ${OS_NAME} ${OS_VERSION} (${OS_BUILD}) on ${ARCH}"
echo -e "  Chip:      ${CHIP}"
echo -e "  Memory:    ${MEMORY_GB} GB"
echo -e "  Rust:      ${RUST_VERSION}"
echo -e "  Git:       ${GIT_HASH}"
echo -e "  Uptime:    ${UPTIME}"
echo ""

# ── Step 0: Build the CLI ────────────────────────────────────────────────

echo -e "${YELLOW}Step 0${RESET} — Building logline-cli (release)..."
cd "$REPO_ROOT"
cargo build --release --bin logline-cli 2>&1 | tail -3
echo -e "  ${GREEN}✓${RESET} Binary ready at ${DIM}${CLI}${RESET}"
echo ""

# ── Step 1: Precondition check ───────────────────────────────────────────

echo -e "${YELLOW}Step 1${RESET} — Precondition check"
echo ""
echo -e "  Before continuing, confirm in the ${BOLD}Supabase Dashboard${RESET}:"
echo -e "    ${CYAN}1.${RESET} All existing auth users are deleted"
echo -e "    ${CYAN}2.${RESET} All app data tables are cleared (run the nuclear SQL)"
echo -e "    ${CYAN}3.${RESET} A new user was created:"
echo -e "       Email:    ${BOLD}${FOUNDER_EMAIL}${RESET}"
echo -e "       Password: ${BOLD}(your choice)${RESET}"
echo -e "       Auto-confirm: ${GREEN}yes${RESET}"
echo ""
read -p "  Have you completed these steps? (yes/no): " CONFIRM
if [[ "$CONFIRM" != "yes" ]]; then
  echo -e "  ${RED}Aborted.${RESET} Complete the Supabase setup first."
  exit 1
fi
echo ""

# ── Step 2: Login ────────────────────────────────────────────────────────

echo -e "${YELLOW}Step 2${RESET} — Logging in as ${BOLD}${FOUNDER_EMAIL}${RESET}"
$CLI auth login --email "$FOUNDER_EMAIL"
echo -e "  ${GREEN}✓${RESET} Authenticated"
echo ""

# ── Step 3: Bootstrap ────────────────────────────────────────────────────

echo -e "${YELLOW}Step 3${RESET} — Bootstrap: creating the world"
echo ""

if [[ -z "${SUPABASE_SERVICE_ROLE_KEY:-}" ]]; then
  echo -e "  ${BOLD}SUPABASE_SERVICE_ROLE_KEY${RESET} not set."
  echo -e "  Find it in: Supabase Dashboard > Settings > API > service_role"
  echo ""
  read -sp "  Paste it here (hidden): " SRK
  echo ""
  export SUPABASE_SERVICE_ROLE_KEY="$SRK"
fi

$CLI founder bootstrap \
  --tenant-slug "$TENANT_SLUG" \
  --tenant-name "$TENANT_NAME"

echo -e "  ${GREEN}✓${RESET} World bootstrapped"
echo ""

# ── Step 4: Register passkey ─────────────────────────────────────────────

echo -e "${YELLOW}Step 4${RESET} — Registering passkey (Touch ID)"
$CLI auth passkey-register --device-name "$COMPUTER_NAME"
echo -e "  ${GREEN}✓${RESET} Passkey registered"
echo ""

# ── Step 5: Re-login with passkey ────────────────────────────────────────

echo -e "${YELLOW}Step 5${RESET} — Re-authenticating via passkey"
$CLI auth login --passkey
echo -e "  ${GREEN}✓${RESET} Identity is now passkey-backed"
echo ""

# ── Step 6: Unlock session ───────────────────────────────────────────────

echo -e "${YELLOW}Step 6${RESET} — Unlocking session (Touch ID)"
$CLI auth session unlock
echo -e "  ${GREEN}✓${RESET} Session active"
echo ""

# ── Step 7: Doctor check ────────────────────────────────────────────────

echo -e "${YELLOW}Step 7${RESET} — Running diagnostics"
$CLI secrets doctor || true
echo ""

# ── Genesis Artifact ─────────────────────────────────────────────────────

USER_ID=$($CLI auth whoami --json 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin).get('id','unknown'))" 2>/dev/null || echo "unknown")

GENESIS_JSON=$(cat <<ARTIFACT
{
  "genesis": {
    "protocol": "logline",
    "version": "0.1.0",
    "record_number": 0,
    "created_at": "${GENESIS_TS}",
    "created_at_local": "${GENESIS_TS_LOCAL}",
    "message": "In the beginning there was a command line."
  },
  "founder": {
    "name": "${FOUNDER_NAME}",
    "email": "${FOUNDER_EMAIL}",
    "user_id": "${USER_ID}",
    "auth_method": "passkey",
    "role": "founder"
  },
  "tenant": {
    "slug": "${TENANT_SLUG}",
    "name": "${TENANT_NAME}"
  },
  "machine": {
    "computer_name": "${COMPUTER_NAME}",
    "hostname": "${HOSTNAME}",
    "username": "${USERNAME}",
    "full_name": "${USER_FULLNAME}",
    "os": "${OS_NAME} ${OS_VERSION}",
    "os_build": "${OS_BUILD}",
    "architecture": "${ARCH}",
    "chip": "${CHIP}",
    "memory_gb": ${MEMORY_GB},
    "kernel": "${KERNEL}",
    "serial_last4": "$(echo "$SERIAL" | tail -c 5)",
    "uptime_at_genesis": "${UPTIME}"
  },
  "toolchain": {
    "rust": "$(echo "$RUST_VERSION" | sed 's/rustc //')",
    "cargo": "$(echo "$CARGO_VERSION" | sed 's/cargo //')",
    "shell": "${SHELL}",
    "git_commit": "${GIT_HASH_FULL}"
  },
  "repositories": {
    "logic": "github.com/danvoulez/logic.logline.world",
    "obs_api": "github.com/danvoulez/obs-api.logline.world",
    "archive": "github.com/danvoulez/LogLine-CLI-UI"
  },
  "oath": "One binary. No secrets on disk. Touch ID or nothing. The CLI is the ecosystem."
}
ARTIFACT
)

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${RESET}"
echo -e "${BOLD}║                                                            ║${RESET}"
echo -e "${BOLD}║   ${GREEN}GENESIS COMPLETE${RESET}${BOLD}                                        ║${RESET}"
echo -e "${BOLD}║   ${DIM}logline.world — record #0${RESET}${BOLD}                                ║${RESET}"
echo -e "${BOLD}║                                                            ║${RESET}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${RESET}"
echo ""
echo "$GENESIS_JSON" | python3 -m json.tool
echo ""

# Save artifact to file
ARTIFACT_PATH="$REPO_ROOT/GENESIS.json"
echo "$GENESIS_JSON" | python3 -m json.tool > "$ARTIFACT_PATH"
echo -e "${DIM}Artifact saved to ${ARTIFACT_PATH}${RESET}"
echo ""
echo -e "${BOLD}In the beginning there was a command line.${RESET}"
echo ""
