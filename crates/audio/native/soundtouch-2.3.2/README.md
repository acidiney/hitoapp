# SoundTouch 2.3.2 declaration headers

These files are copied without modification from `include/` in the `soundtouch-2_3_2/` directory of the pinned [soundtouch-ffi 0.4.1 crate archive](https://crates.io/api/v1/crates/soundtouch-ffi/0.4.1/download). Their original CRLF line endings and copyright/license notices are preserved.

| File | SHA-256 |
| --- | --- |
| FIFOSamplePipe.h | `d290ae4537871772c8863ab82c9a180fab8a95d42fbe2827a8ff89ede03cb9ba` |
| STTypes.h | `9161e896dfaf60ad6fa4cf8ec898624d5f59fb14e179e9c7c6536de8e5b8c564` |
| SoundTouch.h | `742790d93a9acd54b7131915ae717516e18a9d720027106305f1942246123685` |
| soundtouch_config.h | `2356e64a66fc519f4968ed0b692143fa8047c8824b403868c2e546b5524591d1` |

The Rust dependency supplies the matching compiled library. These local headers compile Hito's C ABI adapter against that exact class declaration without relying on dependency-cache paths or guessing C++ layout. Any dependency upgrade must update and verify this header set together.

SoundTouch is Copyright Olli Parviainen and licensed under LGPL-2.1-or-later. See the preserved header notices and the complete [LGPL 2.1 text](../../../../docs/licenses/LGPL-2.1.txt). Distribution requirements and current packaging limits are recorded in [third-party notices](../../../../docs/third-party-notices.md).
