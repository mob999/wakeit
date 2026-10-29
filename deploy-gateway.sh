#!/bin/sh
# Install the independent Gateway; PostgreSQL and NATS are configured separately.
set -eu

usage() {
  cat <<'HELP'
Install Wakeit Gateway on macOS / Linux GNU.

Usage: sh deploy-gateway.sh [--version VERSION] [--bin-dir PATH]
       [--config PATH] [--env-file PATH] [--start]

  --version VERSION  Gateway release (default: 0.4.2)
  --bin-dir PATH     Binary directory (default: $HOME/.local/bin)
  --config PATH      Apply database updates using this existing Gateway config
  --env-file PATH    Load Gateway environment from this file
  --start            Start Gateway in the foreground after applying updates

Stop existing Gateways before using --config for an upgrade. Existing database,
NATS identity and Runner configuration are retained. For automatic startup,
use wakeit-gateway service; for a complete dependency stack, use Docker Compose.
HELP
}
fail() { printf 'Gateway installation failed: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"; }
step() { printf '\n[%s/5] %s\n' "$1" "$2" >&2; }
main() {
  requested=0.4.2
  bin_dir=${HOME:?HOME is required}/.local/bin
  config=
  env_file=
  start=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --version|--bin-dir|--config|--env-file)
        [ "$#" -ge 2 ] || fail "Missing value for $1"
        case "$1" in
          --version) requested=${2#v} ;;
          --bin-dir) bin_dir=$2 ;;
          --config) config=$2 ;;
          --env-file) env_file=$2 ;;
        esac
        shift 2 ;;
      --start) start=true; shift ;;
      -h|--help) usage; return ;;
      *) usage >&2; fail "Unknown argument: $1" ;;
    esac
  done
  [ "$start" = false ] || [ -n "$config" ] || fail '--start requires --config'
  [ -z "$env_file" ] || [ -n "$config" ] || fail '--env-file requires --config'
  need curl
  need awk
  need sed
  printf '%s\n' "$requested" | LC_ALL=C grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail 'Use a stable Gateway version such as 0.4.2'
  step 1 'Detecting platform'
  case "$(uname -m)" in x86_64|amd64) arch=x86_64 ;; arm64|aarch64) arch=aarch64 ;; *) fail 'Unsupported architecture' ;; esac
  case "$(uname -s)" in
    Darwin) target=$arch-apple-darwin ;;
    Linux)
      if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then fail 'The binary requires GNU libc'; fi
      target=$arch-unknown-linux-gnu ;;
    *) fail 'This script supports macOS and Linux GNU; Windows uses the native Gateway package' ;;
  esac
  if command -v sha256sum >/dev/null 2>&1; then hasher=sha256sum
  elif command -v shasum >/dev/null 2>&1; then hasher=shasum
  elif command -v openssl >/dev/null 2>&1; then hasher=openssl
  else fail 'A SHA-256 tool is required'; fi
  umask 077
  temp_dir=$(mktemp -d "${TMPDIR:-/tmp}/wakeit-gateway-install.XXXXXX")
  stage=''
  trap 'rm -rf "$temp_dir"; if [ -n "$stage" ]; then rm -f "$stage"; fi' EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM HUP
  base=https://github.com/mob999/wakeit/releases/download/gateway-v$requested
  asset=wakeit-gateway-$target
  step 2 'Downloading Gateway'
  download "$base/wakeit-gateway-release.json" "$temp_dir/manifest.json"
  version=$(sed -nE 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([0-9]+\.[0-9]+\.[0-9]+)"[[:space:]]*,?[[:space:]]*$/\1/p' "$temp_dir/manifest.json")
  product=$(sed -nE 's/^[[:space:]]*"product"[[:space:]]*:[[:space:]]*"([a-z]+)"[[:space:]]*,?[[:space:]]*$/\1/p' "$temp_dir/manifest.json")
  [ "$product" = gateway ] && [ "$version" = "$requested" ] || fail 'The release manifest does not match the Gateway version'
  download "$base/SHA256SUMS" "$temp_dir/SHA256SUMS"
  download "$base/$asset" "$temp_dir/$asset"
  step 3 'Verifying Gateway'
  verify_checksum "$asset"
  manifest_hash=$(awk -F '"' -v target="$target" '$2 == target { inside=1; next } inside && $2 == "sha256" { print $4; exit } inside && /^[[:space:]]*}/ { exit }' "$temp_dir/manifest.json")
  [ "$manifest_hash" = "$expected" ] || fail 'The manifest and checksum list disagree'
  step 4 'Installing Gateway'
  mkdir -p "$bin_dir"
  bin_dir=$(cd "$bin_dir" && pwd -P)
  stage=$(mktemp "$bin_dir/.wakeit-gateway-install.XXXXXX")
  cp "$temp_dir/$asset" "$stage"
  chmod 755 "$stage"
  reported=$("$stage" --version)
  [ "$reported" = "wakeit-gateway $version" ] || fail 'The executable version does not match the release manifest'
  mv -f "$stage" "$bin_dir/wakeit-gateway"
  stage=''
  step 5 'Preparing Gateway'
  if [ -n "$config" ]; then
    set -- --config "$config"
    if [ -n "$env_file" ]; then set -- "$@" --env-file "$env_file"; fi
    "$bin_dir/wakeit-gateway" migrate "$@"
    if [ "$start" = true ]; then
      rm -rf "$temp_dir"
      trap - EXIT INT TERM HUP
      exec "$bin_dir/wakeit-gateway" start "$@"
    fi
  fi
  printf '\nInstalled Gateway %s: %s/wakeit-gateway\n' "$version" "$bin_dir" >&2
}
download() {
  printf '  %s\n' "${1##*/}" >&2
  curl --fail --show-error --location --progress-bar --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 2 --connect-timeout 15 --output "$2" "$1" || fail "Could not download ${1##*/}"
}
verify_checksum() {
  file=$1
  expected=$(awk -v file="$file" '$2 == file { print $1; count++ } END { if (count != 1) exit 1 }' "$temp_dir/SHA256SUMS") || fail "The checksum entry for $file is missing or duplicated"
  case "$expected" in *[!0-9a-f]*|'') fail 'The release checksum is invalid' ;; esac
  [ "${#expected}" -eq 64 ] || fail 'The release checksum is invalid'
  case "$hasher" in
    sha256sum) actual=$(sha256sum "$temp_dir/$file" | awk '{print $1}') ;;
    shasum) actual=$(shasum -a 256 "$temp_dir/$file" | awk '{print $1}') ;;
    openssl) actual=$(openssl dgst -sha256 "$temp_dir/$file" | awk '{print $NF}') ;;
  esac
  [ "$actual" = "$expected" ] || fail "SHA-256 mismatch for $file; the existing installation was kept"
}
main "$@"
