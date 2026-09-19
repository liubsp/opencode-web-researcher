#!/usr/bin/env bash
set -euo pipefail
project="${1:-}"
ref="${WEB_RESEARCH_REF:-main}"
root="${WEB_RESEARCH_INSTALL_DIR:-$HOME/Library/Application Support/web-research-opencode/app}"
[[ "$(uname -s)" == Darwin ]] || { echo 'This installer supports macOS; use install.ps1 on Windows' >&2; exit 1; }
for tool in node npm cargo curl tar; do command -v "$tool" >/dev/null; done
if [[ -n "$project" ]]; then project="$(cd "$project" && pwd)"; fi
mkdir -p "$root"
mkdir "$root/install.lock.d" 2>/dev/null || { echo 'Another installation is running (or a stale install.lock.d needs review)' >&2; exit 1; }
stage="$(mktemp -d)"
server="$root/bin/web-research-server"
restart=false
cleanup() {
  if $restart && [[ -x "$server" ]]; then "$server" connect >/dev/null 2>&1 || true; fi
  rm -rf "$stage"
  rmdir "$root/install.lock.d"
}
trap cleanup EXIT
curl -fL "https://github.com/liubsp/web-research-opencode/archive/$ref.tar.gz" -o "$stage/source.tar.gz"
mkdir "$stage/source"
tar -xzf "$stage/source.tar.gz" --strip-components=1 -C "$stage/source"
(
  cd "$stage/source"
  cargo build --release --locked -p research-app
  npm ci
  npm run build
  npm pack --workspace web-research-opencode --pack-destination "$stage"
)
if "$stage/source/target/release/web-research" status >/dev/null 2>&1; then
  restart=true
  "$stage/source/target/release/web-research" shutdown
fi
mkdir -p "$root/bin" "$root/runtime"
cp "$stage/source/target/release/web-research" "$server.new"
chmod +x "$server.new"
mv -f "$server.new" "$server"
npm install --prefix "$root/runtime" --omit=dev --no-audit --no-fund "$stage/"*.tgz
"$server" configure
if [[ -n "$project" ]]; then
  node "$root/runtime/node_modules/web-research-opencode/dist/install.js" --project "$project" --binary "$server"
fi
echo "Installed server: $server"
echo 'Reload opted-in OpenCode locations to load the updated plugin. Existing data/login are preserved.'
