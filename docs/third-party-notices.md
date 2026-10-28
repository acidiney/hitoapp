# Native audio third-party notices

The `hito-audio` device feature uses these pinned dependencies for pitch-preserving tempo changes. Builds without that feature do not include them.

| Component | Version | License in supplied source | Source |
| --- | --- | --- | --- |
| Rust SoundTouch bindings | 0.4.1 | LGPL-2.1 | [Cyanistic/soundtouch-ffi](https://github.com/Cyanistic/soundtouch-ffi), [crate archive](https://crates.io/api/v1/crates/soundtouch-ffi/0.4.1/download) |
| Bundled SoundTouch C++ library | 2.3.2 | LGPL-2.1-or-later | `soundtouch-2_3_2/` within the bindings archive, [upstream](https://codeberg.org/soundtouch/soundtouch) |

SoundTouch audio processing library, Copyright Olli Parviainen 2001-2022. The Rust bindings list Cyanism as their author. Their original notices remain in the source archive. Hito does not modify these dependencies.

The complete [GNU Lesser General Public License 2.1](licenses/LGPL-2.1.txt) is copied verbatim from the pinned bindings' `LICENSE` file. SoundTouch is supplied without warranty; see that license for its terms.

`Cargo.lock` records the exact crate versions and archive checksums. The enabled `bundled` feature compiles and statically links the C++ sources through `soundtouch-ffi/build.rs`; libclang generates bindings during the build. Development builds use the Docker desktop tools image and `cargo build --locked -p hito-desktop --features gui`.

Public binary packaging remains unfinished. Distribution must carry the applicable license/notices and corresponding dependency/build sources, with the material needed to rebuild or relink the distributed binary. This source-tree notice is not evidence that installer packaging or those distribution artifacts have been completed.

Hito uses its own small C ABI adapter instead of the `soundtouch` 0.5.4 wrapper, whose destructor is excluded on Windows. C++ constructs and deletes each processor; Rust owns an opaque handle with unconditional Drop. No native exception crosses into Rust. The exact bundled declaration headers and their checksums are recorded in [the header provenance](../crates/audio/native/soundtouch-2.3.2/README.md). They are copied without modification. The opt-in `native-probe` feature counts native construction/destruction for isolated lifecycle regression tests; normal builds omit those counters. Linux tests do not establish Windows runtime acceptance.

## Desktop system media controls

The `hito-desktop` `gui` feature uses `souvlaki` 0.8.3 (MIT, [repository](https://github.com/Sinono3/souvlaki)) for operating-system Now Playing and media keys. Its target-specific dependencies are `dbus-crossroads` (Apache-2.0 or MIT) with the already locked `dbus` on Unix, and `block`, `dispatch`, `objc`, `malloc_buf` (MIT), `cocoa`, `cocoa-foundation`, `core-graphics`, `core-graphics-types`, `foreign-types` (MIT or Apache-2.0) on macOS, and `windows` 0.44 (MIT or Apache-2.0) on Windows. All are permissive and compatible with AGPL-3.0-only. `Cargo.lock` pins them. Hito does not modify them. Public packaging must still carry their license texts. On Linux the `dbus` crate links the system libdbus-1.

## Filesystem watching

`hito-core` uses `notify` 8.2.0 ([repository](https://github.com/notify-rs/notify)) for native folder change events, without its optional crossbeam channel. It is licensed CC0-1.0, which is compatible with AGPL-3.0-only. Its locked dependencies added to `Cargo.lock` are `notify-types` (MIT or Apache-2.0), `inotify` and `inotify-sys` (ISC) on Linux, and `kqueue`, `kqueue-sys` and `fsevent-sys` (MIT) on BSD and macOS. Already locked crates (`mio`, `walkdir`, `bitflags`, `libc`, `log`, `windows-sys`) are reused. Hito does not modify them. Public packaging must still carry their license texts.

## Compressed ID3 frames

`hito-core` uses `flate2` 1.1.10 ([repository](https://github.com/rust-lang/flate2-rs)) to inflate zlib-compressed ID3v2 text frames. It is licensed MIT or Apache-2.0, which is compatible with AGPL-3.0-only. It was already in `Cargo.lock` through other crates, so this adds no new locked package. Its pure Rust backend is used. Hito does not modify it. Public packaging must still carry its license texts.
