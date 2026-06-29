# Deferred Packaging Channels

CBM currently ships via **install scripts**, **GitHub Releases** (multi-platform binaries + `SHA256SUMS.txt`), **Homebrew**, **Scoop**, and **Winget**.

The reference `cbm-mcp` project also lists these channels. They are **intentionally deferred** for the Rust rewrite until wrapper maintenance is automated:

| Channel | Status | Notes |
|---------|--------|-------|
| Go wrapper | Deferred | Thin `main` calling `cbm` binary |
| Python / PyPI | Deferred | `pip install cbm` shim package |
| npm | Deferred | `@cbm/cli` postinstall binary fetch |
| Chocolatey | Deferred | Windows package manager |
| AUR (Arch) | Deferred | `cbm-bin` PKGBUILD |
| Glama MCP registry | Deferred | Metadata publish after stable API |

Supported today:

- `packaging/windows/install.ps1` — checksum-verified download
- `packaging/linux/install.sh` — checksum-verified download
- `packaging/macos/install.sh`
- `packaging/homebrew/cbm.rb`
- `packaging/scoop/cbm.json`
- `packaging/winget/cbm.yaml`

Release hashes are generated in `.github/workflows/release.yml` as `SHA256SUMS.txt`.
