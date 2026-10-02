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
export WEB_RESEARCH_HOME="$data_home"
root="${WEB_RESEARCH_INSTALL_DIR:-$data_home/app}"
[[ "$(uname -s)" == Darwin ]] || { echo 'This installer supports macOS; use install.ps1 on Windows' >&2; exit 1; }
for tool in node npm cargo curl tar; do command -v "$tool" >/dev/null; done
if [[ -n "$project" ]]; then project="$(cd "$project" && pwd)"; fi
mkdir -p "$root"
mkdir "$root/install.lock.d" 2>/dev/null || { echo 'Another installation is running (or a stale install.lock.d needs review)' >&2; exit 1; }
stage="$(mktemp -d)"
legacy_server="$root/bin/opencode-web-researcher"
previous_agent=()
if [[ -f "$root/runtime/node_modules/opencode-web-researcher/agents/web-researcher.md" ]]; then
  cp "$root/runtime/node_modules/opencode-web-researcher/agents/web-researcher.md" "$stage/previous-agent.md"
  previous_agent=(--previous-agent "$stage/previous-agent.md")
fi
legacy_revisions="$root/runtime/node_modules/opencode-web-researcher/dist/updates"
if [[ -d "$legacy_revisions" ]]; then cp -R "$legacy_revisions" "$stage/previous-revisions"; fi
cleanup() {
  if [[ -d "$stage/previous-revisions" ]]; then
    mkdir -p "$legacy_revisions"
    cp -R "$stage/previous-revisions/." "$legacy_revisions/"
  fi
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
candidate="$stage/source/target/release/opencode-web-researcher"
hash="$(shasum -a 256 "$candidate" | cut -d ' ' -f 1)"
server="$root/bin/updates/$hash/opencode-web-researcher"
mkdir -p "$(dirname "$server")" "$root/runtime"
if [[ ! -f "$server" ]]; then cp "$candidate" "$server"; chmod +x "$server"; fi
[[ "$(shasum -a 256 "$server" | cut -d ' ' -f 1)" == "$hash" ]] || { echo 'Installed build differs from candidate' >&2; exit 1; }
npm install --prefix "$root/runtime" --omit=dev --no-audit --no-fund "$stage/"*.tgz
if [[ -d "$stage/previous-revisions" ]]; then
  mkdir -p "$legacy_revisions"
  cp -R "$stage/previous-revisions/." "$legacy_revisions/"
fi
node "$stage/source/scripts/revise-plugin.mjs" "$root/runtime/node_modules/opencode-web-researcher"
"$server" configure
if $global; then
  node "$root/runtime/node_modules/opencode-web-researcher/dist/install.js" --global --binary "$server" --home "$data_home" "${previous_agent[@]}"
elif [[ -n "$project" ]]; then
  node "$root/runtime/node_modules/opencode-web-researcher/dist/install.js" --project "$project" --binary "$server" --home "$data_home" "${previous_agent[@]}"
fi
"$server" activate
cp "$candidate" "$legacy_server.new"
chmod +x "$legacy_server.new"
mv -f "$legacy_server.new" "$legacy_server"
echo "Installed server: $server"
echo 'Reload OpenCode configuration to load updated tools. Already-running calls retain their previous definitions. Existing research data/login are preserved.'
