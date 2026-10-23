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
