# fwx-cli: Samsung firmware extractor

Command-line tool that pulls raw `.img` files out of Samsung firmware packages. It runs on
Windows, macOS, and Linux (Debian and Ubuntu). Give it a firmware file (`.zip`, `.tar`,
`.tar.md5`, or a single `.lz4`), and it writes each lz4 image as a raw `.img`. Android sparse
images are converted to raw automatically.

## Install

Download from the latest GitHub Release. Each release includes `SHA256SUMS.txt` to check your downloads.

**Debian / Ubuntu**

```sh
sudo apt install ./fwx-cli_*_amd64.deb      # or _arm64.deb on ARM
fwx-cli --version
```

**Windows:** unzip `fwx-cli-x86_64-pc-windows-msvc.zip` and run `fwx-cli.exe` from a terminal.

**macOS:** extract the `.tar.gz` for your Mac (`aarch64-apple-darwin` for Apple Silicon,
`x86_64-apple-darwin` for Intel). If macOS blocks the binary because it isn't signed, run
`xattr -d com.apple.quarantine ./fwx-cli` once.

**Linux (any distribution):** extract the `.tar.gz` for your architecture and put `fwx-cli` on your `PATH`.

## Usage

```sh
fwx-cli list    AP_S10PLUS.zip
fwx-cli extract -o ~/firmware AP_S10PLUS.zip
fwx-cli extract -m boot -o ~/firmware AP_S10PLUS.zip      # only images with "boot" in the name
fwx-cli extract -o out AP_1.zip AP_2.zip                  # several firmware files at once
```

Each firmware gets its own `<name>_img` folder under the output root. Without `-o`, the folder is
created next to the firmware file. Re-running overwrites files with the same names.

Exit codes: `0` success, `1` error (for example a missing or corrupt file, or any file in a batch
failing), `2` bad command line.

## Build from source

```sh
python3 tools/make_fixtures.py        # test data; needs: pip install lz4
cargo test --manifest-path rust/Cargo.toml
cargo build --release --manifest-path rust/Cargo.toml --bin fwx-cli
```

The Android app (`app/`) uses the same core through the optional `android` feature.

## Notes

- Scanning a zip reads it sequentially once, so large firmware takes a while.
- Extraction writes to the output folder directly, so keep free space around the size of the largest raw image.
- LZ4 checksums are read past but not verified.
