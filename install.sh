#!/bin/sh
# Install the official Wakeit release without changing services or runtime data.
set -eu

usage() {
  cat <<'HELP'
Install Wakeit CLI and Runner for the current user (macOS / Linux GNU).

Usage: sh install.sh [--version VERSION] [--bin-dir DIRECTORY] [--skill-dir DIRECTORY] [--skill-only] [--no-modify-path]

  --version VERSION  Install a stable release, e.g. 0.2.0 (default: 0.2.0)
  --bin-dir PATH     Install directory (default: $HOME/.local/bin)
  --skill-dir PATH   Install the Wakeit skill at this directory
  --skill-only       Install only the skill; requires --skill-dir
  --no-modify-path   Do not update shell startup files
  -h, --help         Show help
HELP
}
fail() { printf 'Wakeit installation failed: %s\n' "$*" >&2; exit 1; }
step() { printf '\n[%s/5] %s\n' "$1" "$2" >&2; }
need() { command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"; }
quote() { printf "'"; printf '%s' "$1" | sed "s/'/'\\\\''/g"; printf "'"; }

main() {
  requested=0.2.0
  bin_dir=${HOME:?HOME is required}/.local/bin
  modify_path=true
  skill_dir=
  skill_only=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --version|--bin-dir|--skill-dir)
        [ "$#" -ge 2 ] || fail "Missing value for $1"
        case "$1" in --version) requested=${2#v} ;; --bin-dir) bin_dir=$2 ;; --skill-dir) skill_dir=$2 ;; esac
        shift 2 ;;
      --skill-only) skill_only=true; shift ;;
      --no-modify-path) modify_path=false; shift ;;
      -h|--help) usage; return ;;
      *) usage >&2; fail "Unknown argument: $1" ;;
    esac
  done
  if [ "$skill_only" = true ] && [ -z "$skill_dir" ]; then
    fail '--skill-only requires --skill-dir'
  fi
  need curl
  need awk
  need sed
  printf '%s\n' "$requested" | LC_ALL=C grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail 'Use a stable version such as 0.2.0'
  case "$bin_dir" in /*) ;; *) bin_dir="$PWD/$bin_dir" ;; esac
  case "$bin_dir" in *'
'*) fail 'The installation directory cannot contain a newline' ;; esac

  step 1 'Detecting platform'
  machine=$(uname -m)
  case "$machine" in x86_64|amd64) arch=x86_64 ;; arm64|aarch64) arch=aarch64 ;; *) fail "Unsupported architecture: $machine" ;; esac
  case "$(uname -s)" in
    Darwin)
      [ "$arch" = aarch64 ] || fail "Unsupported macOS architecture: $arch"
      target=aarch64-apple-darwin ;;
    Linux)
      if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
        fail 'The official Linux binary requires GNU libc; musl is not supported'
      fi
      target=$arch-unknown-linux-gnu ;;
    *) fail 'This script supports macOS and Linux GNU. Windows: https://github.com/mob999/wakeit/blob/main/docs/install.md' ;;
  esac
  printf '  %s\n' "$target" >&2
  if command -v sha256sum >/dev/null 2>&1; then hasher=sha256sum
  elif command -v shasum >/dev/null 2>&1; then hasher=shasum
  elif command -v openssl >/dev/null 2>&1; then hasher=openssl
  else fail 'A SHA-256 tool is required (sha256sum, shasum or openssl)'; fi

  umask 077
  temp_dir=$(mktemp -d "${TMPDIR:-/tmp}/wakeit-install.XXXXXX") || fail 'Cannot create a temporary directory'
  stage=''
  trap 'rm -rf "$temp_dir"; if [ -n "$stage" ]; then rm -f "$stage"; fi' EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM HUP
  release_root=https://github.com/mob999/wakeit/releases
  metadata_url=$release_root/download/cli-v$requested/wakeit-cli-release.json

  step 2 'Downloading release files'
  download "$metadata_url" "$temp_dir/manifest.json"
  # package-release.py emits one top-level stable version and target asset blocks.
  version=$(sed -nE 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([0-9]+\.[0-9]+\.[0-9]+)"[[:space:]]*,?[[:space:]]*$/\1/p' "$temp_dir/manifest.json")
  [ -n "$version" ] || fail 'The release manifest has no stable version'
  [ "$requested" = "$version" ] || fail 'The release manifest does not match the requested version'
  product=$(sed -nE 's/^[[:space:]]*"product"[[:space:]]*:[[:space:]]*"([a-z]+)"[[:space:]]*,?[[:space:]]*$/\1/p' "$temp_dir/manifest.json")
  [ "$product" = cli ] || fail 'The release manifest is not a CLI release'
  base=$release_root/download/cli-v$version
  asset=wakeit-$target.tar.gz
  download "$base/SHA256SUMS" "$temp_dir/SHA256SUMS"
  if [ "$skill_only" = false ]; then
    need tar
    need diff
    download "$base/$asset" "$temp_dir/$asset"
  fi

  if [ -n "$skill_dir" ]; then
    need tar
    need diff
    download "$base/wakeit-skill.tar.gz" "$temp_dir/wakeit-skill.tar.gz"
  fi

  step 3 'Verifying release files'
  if [ "$skill_only" = false ]; then
    verify_checksum "$asset"
    manifest_hash=$(awk -F '"' -v target="$target" '$2 == target { inside=1; next } inside && $2 == "sha256" { print $4; exit } inside && /^[[:space:]]*}/ { exit }' "$temp_dir/manifest.json")
    [ "$manifest_hash" = "$expected" ] || fail 'The manifest and checksum list disagree'
    tar -tzf "$temp_dir/$asset" > "$temp_dir/cli-files" || fail 'The CLI archive is invalid'
    awk -v version="$version" '$0 != "wakeit" && index($0, "wakeit-plugins/" version "/") != 1 { exit 1 } /(^|\/)\.\.(\/|$)/ { exit 1 }' "$temp_dir/cli-files" || fail 'The CLI archive contains unsafe paths'
    tar -tvzf "$temp_dir/$asset" | awk 'substr($0,1,1) !~ /[-d]/ { exit 1 }' || fail 'The CLI archive contains unsupported links or special files'
    mkdir "$temp_dir/cli"
    tar -xzf "$temp_dir/$asset" -C "$temp_dir/cli" || fail 'Cannot unpack the CLI'
    bundle="$temp_dir/cli/wakeit-plugins/$version"
    "$temp_dir/cli/wakeit" --json plugin validate --dir "$bundle" >/dev/null || fail 'The CLI archive contains an incomplete plugin bundle'

  fi
  if [ -n "$skill_dir" ]; then
    verify_checksum wakeit-skill.tar.gz
    tar -tzf "$temp_dir/wakeit-skill.tar.gz" > "$temp_dir/skill-files" || fail 'The skill archive is invalid'
    awk '$0 != "wakeit" && $0 !~ /^wakeit\// { exit 1 } /(^|\/)\.\.(\/|$)/ { exit 1 }' "$temp_dir/skill-files" || fail 'The skill archive contains unsafe paths'
    tar -tvzf "$temp_dir/wakeit-skill.tar.gz" | awk 'substr($0,1,1) !~ /[-d]/ { exit 1 }' || fail 'The skill archive contains unsupported links or special files'
    mkdir "$temp_dir/skill"
    tar -xzf "$temp_dir/wakeit-skill.tar.gz" -C "$temp_dir/skill" || fail 'Cannot unpack the skill'
    [ -f "$temp_dir/skill/wakeit/SKILL.md" ] || fail 'The skill archive is incomplete'
    if [ -e "$skill_dir" ] || [ -L "$skill_dir" ]; then
      [ ! -L "$skill_dir" ] && [ -d "$skill_dir" ] && diff -qr "$temp_dir/skill/wakeit" "$skill_dir" >/dev/null 2>&1 || fail 'The skill directory contains different files; local customizations were kept'
    fi
  fi

  step 4 'Installing files'
  if [ "$skill_only" = false ]; then
    mkdir -p "$bin_dir" || fail "Cannot create $bin_dir"
    bin_dir=$(cd "$bin_dir" && pwd -P)
    [ ! -d "$bin_dir/wakeit" ] || fail "$bin_dir/wakeit is a directory"
    stage=$(mktemp "$bin_dir/.wakeit-install.XXXXXX") || fail "Cannot write to $bin_dir"
    cp "$temp_dir/cli/wakeit" "$stage" || fail 'Cannot stage the executable'
    chmod 755 "$stage"
    reported=$("$stage" --version) || fail 'The downloaded binary cannot run on this system'
    [ "$reported" = "wakeit $version" ] || fail 'The executable version does not match the release manifest'
    mkdir -p "$bin_dir/wakeit-plugins" || fail 'Cannot create the plugin directory'
    installed_bundle="$bin_dir/wakeit-plugins/$version"
    if [ -e "$installed_bundle" ]; then
      diff -qr "$bundle" "$installed_bundle" >/dev/null 2>&1 || fail 'A different plugin bundle is already installed for this version'
    else
      mv "$bundle" "$installed_bundle" || fail 'Cannot install the official adapters'
    fi
    mv -f "$stage" "$bin_dir/wakeit" || fail 'Cannot replace the executable; the current installation was kept'
    stage=''
    printf '  %s\n' "$bin_dir/wakeit" >&2
  fi
  if [ -n "$skill_dir" ] && [ ! -d "$skill_dir" ]; then
    mkdir -p "$(dirname "$skill_dir")" || fail 'Cannot create the skill parent directory'
    mv "$temp_dir/skill/wakeit" "$skill_dir" || fail 'Cannot install the skill'
    printf '  Skill: %s\n' "$skill_dir" >&2
  fi

  if [ "$skill_only" = true ]; then
    step 5 'Skill ready'
    printf '\nInstalled Wakeit skill %s: %s\n' "$version" "$skill_dir" >&2
    return
  fi
  step 5 'Configuring command access'
  path_configured=false
  if [ "$modify_path" = true ]; then
    path_configured=true
    shell_path=${SHELL:-}
    if [ -z "$shell_path" ]; then
      if command -v getent >/dev/null 2>&1; then shell_path=$(getent passwd "$(id -un)" | awk -F: '{print $7}')
      elif command -v dscl >/dev/null 2>&1; then shell_path=$(dscl . -read "/Users/$(id -un)" UserShell | awk '{print $2}'); fi
    fi
    case "${shell_path##*/}" in
      zsh) configure_posix "${ZDOTDIR:-$HOME}/.zshrc"; configure_posix "${ZDOTDIR:-$HOME}/.zprofile" ;;
      bash)
        configure_posix "$HOME/.bashrc"
        if [ -f "$HOME/.bash_profile" ]; then configure_posix "$HOME/.bash_profile"
        elif [ -f "$HOME/.bash_login" ]; then configure_posix "$HOME/.bash_login"
        else configure_posix "$HOME/.profile"; fi ;;
      fish)
        fish_file=${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/wakeit.fish
        mkdir -p "$(dirname "$fish_file")" || fail 'Cannot create the fish configuration directory'
        quoted=$(quote "$bin_dir")
        marker="# Wakeit PATH: $bin_dir"
        if [ ! -f "$fish_file" ] || ! grep -Fqx "$marker" "$fish_file"; then
          printf '\n%s\nif not contains -- %s $PATH\n  set -gx PATH %s $PATH\nend\n' "$marker" "$quoted" "$quoted" >> "$fish_file" || fail 'Cannot configure fish PATH'
        fi
        printf '  PATH: %s\n' "$fish_file" >&2 ;;
      sh|dash|ksh|'') configure_posix "$HOME/.profile" ;;
      *) path_configured=false; printf '  Shell %s: add %s to your PATH.\n' "$shell_path" "$bin_dir" >&2 ;;
    esac
  else
    printf '  Shell configuration left unchanged.\n' >&2
  fi
  printf '\nInstalled Wakeit %s.\n' "$version" >&2
  if [ "$path_configured" = true ]; then
    printf 'Open a new terminal and run: wakeit --version\n' >&2
  else
    printf 'Add %s to PATH, or run the executable below directly.\n' "$bin_dir" >&2
  fi
  printf 'Executable: %s/wakeit\n' "$bin_dir" >&2
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
configure_posix() {
  config_file=$1
  marker="# Wakeit PATH: $bin_dir"
  mkdir -p "$(dirname "$config_file")" || fail "Cannot create the directory for $config_file"
  if [ ! -f "$config_file" ] || ! grep -Fqx "$marker" "$config_file"; then
    quoted=$(quote "$bin_dir")
    quoted_component=$(quote ":$bin_dir:")
    printf '\n%s\ncase ":$PATH:" in\n  *%s*) ;;\n  *) export PATH=%s:"$PATH" ;;\nesac\n' "$marker" "$quoted_component" "$quoted" >> "$config_file" || fail "Cannot update $config_file (the executable is installed)"
  fi
  printf '  PATH: %s\n' "$config_file" >&2
}

# Read all source before starting, so piping curl to sh never competes for stdin.
main "$@"
