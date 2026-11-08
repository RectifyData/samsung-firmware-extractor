# Samsung Firmware Extractor

Android app (Jetpack Compose UI) that pulls raw `.img` files out of Samsung firmware.
Pick one or more firmware files (`.zip`, `.tar`, `.tar.md5`), tick the `.lz4` images you want,
and pick an output folder. Each firmware gets its own `<name>_img` subfolder. Android sparse
images are converted to raw automatically.

## Layout

- `rust/` core library (`fwx`): LZ4 frame decoder, zip/tar walker (nested containers included),
  sparse-to-raw conversion, and the JNI bridge. Pure Rust, no C dependencies.
- `app/` Jetpack Compose UI and the Kotlin bridge (`Native.kt`) that loads `libfwx.so`.
- `tools/make_fixtures.py` builds synthetic firmware for the Rust round-trip tests.
- `.github/workflows/android.yml` runs the Rust tests, cross-compiles the core with
  cargo-ndk, and builds the debug APK.

## Build

CI builds the APK on every push. The artifact is `firmware-extractor-debug-apk`.

Locally:

```sh
python3 tools/make_fixtures.py          # needs: pip install lz4
cargo test --manifest-path rust/Cargo.toml
cargo ndk -t arm64-v8a -t x86_64 -o app/src/main/jniLibs build --release --manifest-path rust/Cargo.toml
gradle :app:assembleDebug               # Android Studio works too
```

## Notes

- Scanning a zip reads it sequentially once. This is slower than a desktop tool with random access.
- Decompressed images are written to the app cache before they're copied to the output folder,
  so keep free space around the size of the largest raw image.
- LZ4 checksums are read past but not verified.
