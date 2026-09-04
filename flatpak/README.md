# Simplevoice on Flathub

This directory contains upstream Flatpak metadata and the source-only manifest
for `io.github.MaciejKolerski.simplevoice`.

## Current bootstrap state

The manifest currently points at `v0.1.9` so its dependency lists are pinned and
reviewable, but that tag predates this directory. Do not submit it to Flathub
yet: the first eligible tag must contain the desktop file, MetaInfo file,
manifest, and downstream patch from this directory.

Before creating that tag:

1. Keep the versions in `package.json`, `src-tauri/Cargo.toml`, and
   `src-tauri/tauri.conf.json` identical.
2. Add a concise, user-facing release entry to
   `io.github.MaciejKolerski.simplevoice.metainfo.xml`.
3. Commit and push the packaging files together with the release source.
4. Create and push the stable `vX.Y.Z` tag.
5. Run `./flatpak/prepare-release.sh vX.Y.Z`. This verifies the remote tag,
   regenerates the offline Cargo and pnpm source lists, and pins the manifest to
   the tagged commit.

## Validate and test locally

Install the current GNOME SDK and Flathub's Builder utility, then run:

```sh
cd flatpak
git clone --filter=blob:none https://github.com/flathub/shared-modules.git
git -C shared-modules checkout cb9ec602a1ece1c76d5a4f8aa1d87c4a6bf99c3e
flatpak install --user flathub org.gnome.Platform//50 org.gnome.Sdk//50 org.flatpak.Builder
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest io.github.MaciejKolerski.simplevoice.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder appstream io.github.MaciejKolerski.simplevoice.metainfo.xml
flatpak run --command=flathub-build org.flatpak.Builder --install io.github.MaciejKolerski.simplevoice.yml
flatpak run io.github.MaciejKolerski.simplevoice
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
```

The Flatpak build intentionally uses the local Candle/Whisper CPU engine. The
current `sherpa-onnx-sys` build script downloads precompiled native libraries,
which Flathub does not permit, and the Vulkan shader compiler is not included in
the runtime SDK. ONNX download suggestions and the GPU setting are hidden in
the Flatpak interface. ONNX and Vulkan remain available in the native packages.

Autostart is hidden in the Flatpak build because the native Tauri autostart
plugin cannot manage a sandboxed application. Updates are handled by Flatpak,
so the in-app update dialog shows the correct `flatpak update` command.

## Initial submission (must be performed by a human)

Flathub requires a human to open the initial pull request from the GitHub web
interface. AI tools must not open or automate it and must not write its commit
message, PR description, review comments, or replies.

Fork `flathub/flathub` with all branches, branch from `new-pr`, and place these
regular files at the top level of the submission branch:

- `io.github.MaciejKolerski.simplevoice.yml`
- `disable-onnx-vulkan.patch`
- `cargo-sources.json`
- `node-sources.json`

Then add Flathub's shared modules as a pinned git submodule (this also creates
the required `.gitmodules` file):

```sh
git submodule add https://github.com/flathub/shared-modules.git shared-modules
git -C shared-modules checkout cb9ec602a1ece1c76d5a4f8aa1d87c4a6bf99c3e
git add .gitmodules shared-modules
```

The desktop and MetaInfo files stay in the upstream source; Flathub explicitly
requires upstream metadata rather than copies in the submission repository.
Write the commit message and submission text yourself. The submission must also
disclose that Codex generated most of the Flatpak packaging and identify the
affected files and approximate extent. Target the `new-pr` branch, not `master`.

The sandbox cannot read `/dev/input` or edit the host compositor configuration,
so shortcut settings are hidden in this Flatpak build. Recording remains
available from the always-visible in-app control and the tray menu. Do not work
around this with broad device or home-directory permissions; use the XDG
GlobalShortcuts portal in a future application update if sandboxed global
shortcuts are required.

## Automatic updates after acceptance

After Flathub creates `flathub/io.github.MaciejKolerski.simplevoice`:

1. Enable GitHub 2FA and accept the Flathub repository invitation within one
   week.
2. In that dedicated repository, copy `maintenance/update.yml` to
   `.github/workflows/update.yml`, and copy `maintenance/flathub.json` to the
   repository root. Submit these maintenance changes through its normal PR
   workflow.
3. Create a personal GitHub token that can send repository dispatch events to
   `flathub/io.github.MaciejKolerski.simplevoice` (`Contents: write` for that
   repository). Store it as the `FLATHUB_TOKEN` Actions secret in the upstream
   `MaciejKolerski/simplevoice` repository.
4. Run the maintenance workflow once manually with the current release tag.

After that, every successful upstream release sends `simplevoice-release` to
the Flathub repository. Its workflow regenerates all offline sources and opens
an update PR. A successful Flathub build plus merging that PR publishes the
update. Fully unattended merging is available through the
`FLATHUB_AUTO_MERGE=true` repository variable only after Flathub grants an
automerge exception; testing the generated Flatpak before merge is the safer
default.
