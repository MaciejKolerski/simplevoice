# AUR packages

PKGBUILD templates for two AUR packages:

- **`simplevoice-bin`** — installs the prebuilt `.deb` from the GitHub release.
- **`simplevoice`** — builds from source from the `vX.Y.Z` tag.

The `publish-aur` job in `.github/workflows/release.yml` bumps the version and
checksums and pushes both packages to the AUR on every `vX.Y.Z` tag.

## One-time setup

1. **CI SSH key.** Generate a keypair. Add the **public** key to your AUR account
   (*My Account → SSH Public Key*) and the **private** key as the repository
   secret `AUR_SSH_PRIVATE_KEY` (*Settings → Secrets and variables → Actions*).

2. **First import** (once per package, on an Arch machine). CI only handles
   updates — it does not create the package:

   ```bash
   git clone ssh://aur@aur.archlinux.org/simplevoice-bin.git
   cd simplevoice-bin
   cp /path/to/repo/aur/simplevoice-bin/{PKGBUILD,simplevoice-bin.install} .
   updpkgsums
   makepkg -si
   makepkg --printsrcinfo > .SRCINFO
   git add -A && git commit -m "Initial import" && git push origin master
   ```

The `.deb` filename is assumed to be `Simplevoice_<version>_amd64.deb`. If the
release asset differs, fix `source_x86_64` in `aur/simplevoice-bin/PKGBUILD` and
the matching URL in the `prepare simplevoice-bin PKGBUILD` step of `release.yml`.
