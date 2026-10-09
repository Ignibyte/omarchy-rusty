# Packaging

Two Arch packages, neither on the AUR yet:

- `PKGBUILD` builds `rusty-git` from the main branch: the three binaries (`rusty-mcp`,
  `rusty-cli`, `rusty`), the back end's user unit pointed at `/usr/bin`, the compositor
  drop-in and MCP snippets under `/usr/share/rusty/`, the licence, the third-party notices
  and the README. Its `check()` runs the tests of `rusty-core`, `rusty-mcp` and
  `rusty-cmd`.
- `rusty-bin/PKGBUILD` installs the same files from a release's prebuilt tarball. Its
  `sha256sums` stay `SKIP` until a release exists: after each release, set `pkgver` and
  run `updpkgsums` so the download is verified.

Build them from the repository root:

```bash
(cd packaging && makepkg -sf)             # rusty-git; add -i to install
(cd packaging/rusty-bin && makepkg -sf)   # rusty-bin for the pkgver it names
```

A package cannot start a user's service. After installing, and again after every
upgrade, run as your user:

```bash
systemctl --user daemon-reload
systemctl --user enable --now rusty-mcp    # the first time
systemctl --user restart rusty-mcp         # after an upgrade
```

`omarchy/install.sh` is the path for a checkout or an unpacked tarball without a package.
