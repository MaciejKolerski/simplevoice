#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! "$1" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "Usage: $0 vX.Y.Z" >&2
  exit 2
fi

readonly release_tag="$1"
readonly version="${release_tag#v}"
readonly upstream_url="https://github.com/MaciejKolerski/simplevoice.git"
readonly tools_commit="1fc32195e3e60fe5c97f0af646dec7a99df5962b"
repo_root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
readonly repo_root
work_dir=$(mktemp -d /tmp/simplevoice-flatpak.XXXXXX)
readonly work_dir
trap 'rm -rf -- "$work_dir"' EXIT

remote_commit=$(git ls-remote "$upstream_url" "refs/tags/${release_tag}^{}" | cut -f1)
if [[ -z "$remote_commit" ]]; then
  remote_commit=$(git ls-remote "$upstream_url" "refs/tags/${release_tag}" | cut -f1)
fi
if [[ -z "$remote_commit" ]]; then
  echo "Tag $release_tag is not available in the upstream GitHub repository." >&2
  exit 1
fi

git clone --quiet --depth 1 --branch "$release_tag" "$upstream_url" "$work_dir/source"
actual_commit=$(git -C "$work_dir/source" rev-parse HEAD)
if [[ "$actual_commit" != "$remote_commit" ]]; then
  echo "Resolved tag commit $actual_commit differs from GitHub's $remote_commit." >&2
  exit 1
fi

for version_file in package.json src-tauri/tauri.conf.json; do
  actual_version=$(jq -r .version "$work_dir/source/$version_file")
  if [[ "$actual_version" != "$version" ]]; then
    echo "$version_file has version $actual_version, expected $version." >&2
    exit 1
  fi
done
cargo_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$work_dir/source/src-tauri/Cargo.toml" | head -n 1)
if [[ "$cargo_version" != "$version" ]]; then
  echo "src-tauri/Cargo.toml has version $cargo_version, expected $version." >&2
  exit 1
fi

for required_file in \
  flatpak/io.github.MaciejKolerski.simplevoice.desktop \
  flatpak/io.github.MaciejKolerski.simplevoice.metainfo.xml \
  flatpak/io.github.MaciejKolerski.simplevoice.yml \
  flatpak/disable-onnx-vulkan.patch; do
  if [[ ! -f "$work_dir/source/$required_file" ]]; then
    echo "$release_tag does not contain $required_file; create a newer release first." >&2
    exit 1
  fi
done

git -C "$work_dir/source" apply --check flatpak/disable-onnx-vulkan.patch

if ! grep -Fq "<release version=\"$version\"" \
  "$work_dir/source/flatpak/io.github.MaciejKolerski.simplevoice.metainfo.xml"; then
  echo "The MetaInfo file needs a release entry for $version before tagging." >&2
  exit 1
fi

git clone --quiet https://github.com/flatpak/flatpak-builder-tools.git "$work_dir/tools"
git -C "$work_dir/tools" checkout --quiet "$tools_commit"
python3 -m venv "$work_dir/venv"
"$work_dir/venv/bin/pip" install --quiet "$work_dir/tools/node" aiohttp tomlkit

"$work_dir/venv/bin/flatpak-node-generator" \
  --no-requests-cache \
  --retries 3 \
  --output "$work_dir/node-sources.json" \
  pnpm "$work_dir/source/pnpm-lock.yaml"
"$work_dir/venv/bin/python" "$work_dir/tools/cargo/flatpak-cargo-generator.py" \
  --output "$work_dir/cargo-sources.json" \
  "$work_dir/source/src-tauri/Cargo.lock"

# Playwright is used only by screenshot tooling. Keep its npm packages (the
# frozen install needs them), but omit the large browser archives and suppress
# their install script during the Flatpak build.
jq 'map(select((((.dest // "") | startswith("flatpak-node/cache/ms-playwright"))) | not))' \
  "$work_dir/node-sources.json" > "$work_dir/node-sources.filtered.json"

cp "$repo_root/flatpak/io.github.MaciejKolerski.simplevoice.yml" "$work_dir/manifest.yml"
python3 - "$work_dir/manifest.yml" \
  "$release_tag" "$remote_commit" <<'PY'
from pathlib import Path
import re
import sys

manifest = Path(sys.argv[1])
tag = sys.argv[2]
commit = sys.argv[3]
text = manifest.read_text()
pattern = re.compile(
    r"(?P<prefix>\n\s+- type: git\n"
    r"\s+url: https://github\.com/MaciejKolerski/simplevoice\.git\n"
    r"\s+tag: )\S+"
    r"(?P<middle>\n\s+commit: )[0-9a-f]+"
)
updated, count = pattern.subn(
    lambda match: f"{match.group('prefix')}{tag}{match.group('middle')}{commit}",
    text,
    count=1,
)
if count != 1:
    raise SystemExit("Could not locate the Simplevoice source block in the manifest")
manifest.write_text(updated)
PY

# Do not leave a half-refreshed package if generation or manifest validation
# fails. Publish all three prepared files only after every step above succeeds.
install -m 0644 "$work_dir/cargo-sources.json" "$repo_root/flatpak/cargo-sources.json"
install -m 0644 "$work_dir/node-sources.filtered.json" "$repo_root/flatpak/node-sources.json"
install -m 0644 "$work_dir/manifest.yml" "$repo_root/flatpak/io.github.MaciejKolerski.simplevoice.yml"

echo "Prepared Flatpak sources for $release_tag ($remote_commit)."
echo "Next: validate and build using the commands in flatpak/README.md."
