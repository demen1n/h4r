# Third-party dependency inventory

Checked on 2026-10-31 against the exact versions in `Cargo.lock`, using
`cargo metadata --locked --format-version 1` and locally downloaded crate sources.
The inventory includes target-specific packages, not just the current macOS build.

All 37 external packages declare a GPLv3-compatible permissive license option.
For this review, MIT can be selected wherever it is offered; `foldhash` and
`zlib-rs` use Zlib. Older `MIT/Apache-2.0` metadata denotes alternative licenses.
These packages retain their upstream licenses; they are not relicensed by h4r.

| Package | Version | Declared license |
| --- | --- | --- |
| [adler2](https://crates.io/crates/adler2/2.0.1) | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| [allocator-api2](https://crates.io/crates/allocator-api2/0.2.21) | 0.2.21 | MIT OR Apache-2.0 |
| [autocfg](https://crates.io/crates/autocfg/1.5.1) | 1.5.1 | Apache-2.0 OR MIT |
| [bitflags](https://crates.io/crates/bitflags/1.3.2) | 1.3.2 | MIT/Apache-2.0 |
| [bytemuck](https://crates.io/crates/bytemuck/1.25.2) | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| [byteorder](https://crates.io/crates/byteorder/1.5.0) | 1.5.0 | Unlicense OR MIT |
| [cfg-if](https://crates.io/crates/cfg-if/1.0.5) | 1.0.5 | MIT OR Apache-2.0 |
| [color_quant](https://crates.io/crates/color_quant/1.1.0) | 1.1.0 | MIT |
| [core_maths](https://crates.io/crates/core_maths/0.1.1) | 0.1.1 | MIT |
| [crc32fast](https://crates.io/crates/crc32fast/1.5.3) | 1.5.3 | MIT OR Apache-2.0 |
| [equivalent](https://crates.io/crates/equivalent/1.0.3) | 1.0.3 | Apache-2.0 OR MIT |
| [fdeflate](https://crates.io/crates/fdeflate/0.3.7) | 0.3.7 | MIT OR Apache-2.0 |
| [flate2](https://crates.io/crates/flate2/1.1.10) | 1.1.10 | MIT OR Apache-2.0 |
| [foldhash](https://crates.io/crates/foldhash/0.1.5) | 0.1.5 | Zlib |
| [fontdue](https://crates.io/crates/fontdue/0.9.5) | 0.9.5 | MIT OR Apache-2.0 OR Zlib |
| [glam](https://crates.io/crates/glam/0.27.0) | 0.27.0 | MIT OR Apache-2.0 |
| [hashbrown](https://crates.io/crates/hashbrown/0.15.5) | 0.15.5 | MIT OR Apache-2.0 |
| [image](https://crates.io/crates/image/0.24.9) | 0.24.9 | MIT OR Apache-2.0 |
| [libc](https://crates.io/crates/libc/0.2.191) | 0.2.191 | MIT OR Apache-2.0 |
| [libm](https://crates.io/crates/libm/0.2.16) | 0.2.16 | MIT |
| [macroquad](https://crates.io/crates/macroquad/0.4.16) | 0.4.16 | MIT OR Apache-2.0 |
| [macroquad_macro](https://crates.io/crates/macroquad_macro/0.1.8) | 0.1.8 | MIT/Apache-2.0 |
| [malloc_buf](https://crates.io/crates/malloc_buf/0.0.6) | 0.0.6 | MIT |
| [miniquad](https://crates.io/crates/miniquad/0.4.11) | 0.4.11 | MIT OR Apache-2.0 |
| [miniz_oxide](https://crates.io/crates/miniz_oxide/0.8.9) | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| [miniz_oxide](https://crates.io/crates/miniz_oxide/0.9.1) | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| [ndk-sys](https://crates.io/crates/ndk-sys/0.2.2) | 0.2.2 | MIT OR Apache-2.0 |
| [num-traits](https://crates.io/crates/num-traits/0.2.19) | 0.2.19 | MIT OR Apache-2.0 |
| [objc-rs](https://crates.io/crates/objc-rs/0.2.8) | 0.2.8 | MIT |
| [png](https://crates.io/crates/png/0.17.16) | 0.17.16 | MIT OR Apache-2.0 |
| [quad-rand](https://crates.io/crates/quad-rand/0.2.3) | 0.2.3 | MIT |
| [simd-adler32](https://crates.io/crates/simd-adler32/0.3.10) | 0.3.10 | MIT |
| [ttf-parser](https://crates.io/crates/ttf-parser/0.25.1) | 0.25.1 | MIT OR Apache-2.0 |
| [winapi](https://crates.io/crates/winapi/0.3.9) | 0.3.9 | MIT/Apache-2.0 |
| [winapi-i686-pc-windows-gnu](https://crates.io/crates/winapi-i686-pc-windows-gnu/0.4.0) | 0.4.0 | MIT/Apache-2.0 |
| [winapi-x86_64-pc-windows-gnu](https://crates.io/crates/winapi-x86_64-pc-windows-gnu/0.4.0) | 0.4.0 | MIT/Apache-2.0 |
| [zlib-rs](https://crates.io/crates/zlib-rs/0.6.8) | 0.6.8 | Zlib |

## Embedded material and distribution notices

Macroquad embeds `ProggyClean.ttf`, by Tristan Grimmer. The
[official Proggy fonts project](https://github.com/bluescan/proggyfonts)
publishes the original fonts under MIT. Example assets shipped in dependency
source archives are not automatically part of h4r binaries.

This table is an inventory, not a complete distribution notice bundle. When
shipping binaries or vendored dependency sources, include the applicable
upstream copyright notices and license texts, including embedded material,
and satisfy GPLv3 source-distribution requirements for the build being shipped.

Some crate archives (`malloc_buf`, `quad-rand`, `ndk-sys`, and the two Windows
import-library packages) declare licenses in Cargo metadata but omit a separate
license file. Resolve the corresponding upstream notices when preparing a
binary distribution; their permissive metadata is not itself a notice bundle.

## Compatibility references

- [GNU license list](https://www.gnu.org/licenses/license-list.en.html)
- [Apache Software Foundation: Apache 2.0 and GPLv3 compatibility](https://apache.org/licenses/GPL-compatibility.html)

This review checks declared dependency terms and packaged license material.
It does not certify the provenance of every upstream source line or replace
an assessment of the original game and its reverse engineering.
