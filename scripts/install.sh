#!/usr/bin/env bash
set -euo pipefail
project="${1:-}"
global=false
if [[ "$project" == --global ]]; then global=true; project=""; fi
[[ $# -le 1 ]] || { echo 'Usage: install.sh [--global | project-directory]' >&2; exit 1; }
ref="${WEB_RESEARCH_REF:-main}"
data_home="$HOME/Library/Application Support/opencode-web-researcher"
if [[ -d "$HOME/Library/Application Support/web-research-opencode" ]]; then
  data_home="$HOME/Library/Application Support/web-research-opencode"
fi
data_home="${WEB_RESEARCH_HOME:-$data_home}"
root="${WEB_RESEARCH_INSTALL_DIR:-$data_home/app}"
[[ "$(uname -s)" == Darwin ]] || { echo 'This installer supports macOS; use install.ps1 on Windows' >&2; exit 1; }
for tool in node npm cargo curl tar; do command -v "$tool" >/dev/null; done
if [[ -n "$project" ]]; then project="$(cd "$project" && pwd)"; fi
mkdir -p "$root"
mkdir "$root/install.lock.d" 2>/dev/null || { echo 'Another installation is running (or a stale install.lock.d needs review)' >&2; exit 1; }
stage="$(mktemp -d)"
server="$root/bin/opencode-web-researcher"
restart=false
cleanup() {
  if $restart && [[ -x "$server" ]]; then "$server" connect >/dev/null 2>&1 || true; fi
  rm -rf "$stage"
  rmdir "$root/install.lock.d"
}
trap cleanup EXIT
curl -fL "https://github.com/liubsp/opencode-web-researcher/archive/$ref.tar.gz" -o "$stage/source.tar.gz"
mkdir "$stage/source"
tar -xzf "$stage/source.tar.gz" --strip-components=1 -C "$stage/source"
(
  cd "$stage/source"
  node scripts/build-release.mjs
  npm ci
  npm run build
  npm pack --workspace opencode-web-researcher --pack-destination "$stage"
)
if "$stage/source/target/release/opencode-web-researcher" status >/dev/null 2>&1; then
  restart=true
  "$stage/source/target/release/opencode-web-researcher" shutdown
fi
mkdir -p "$root/bin" "$root/runtime"
cp "$stage/source/target/release/opencode-web-researcher" "$server.new"
chmod +x "$server.new"
mv -f "$server.new" "$server"
npm install --prefix "$root/runtime" --omit=dev --no-audit --no-fund "$stage/"*.tgz
"$server" configure
if $global; then
  node "$root/runtime/node_modules/opencode-web-researcher/dist/install.js" --global --binary "$server"
elif [[ -n "$project" ]]; then
  node "$root/runtime/node_modules/opencode-web-researcher/dist/install.js" --project "$project" --binary "$server"
fi
echo "Installed server: $server"
echo 'Reload opted-in OpenCode locations to load the updated plugin. Existing data/login are preserved.'
