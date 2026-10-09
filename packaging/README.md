# Packaging

`PKGBUILD` builds `rusty-git` from the main branch: the three binaries (`rusty-mcp`,
`rusty-cli`, `rusty`), the back end's user unit (pointed at `/usr/bin`), the compositor
drop-in and MCP snippets, the licence and the README. It is validated with `makepkg` and is
not on the AUR yet. `omarchy/install.sh` remains the from-source path for a checkout.

`rusty-bin/PKGBUILD` packages a release's prebuilt binaries instead; after each release,
set its `pkgver` and run `updpkgsums`.

```bash
cd packaging && makepkg -sf             # rusty-git from main; add -i to install
cd packaging/rusty-bin && makepkg -sf   # rusty-bin from the latest release
```
