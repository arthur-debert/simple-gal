# System Dependencies

Simple Gal has **no system-level dependencies**. All image processing (JPEG/PNG/TIFF/WebP decode, AVIF encode, resize, IPTC metadata extraction) is handled by pure Rust libraries compiled into the binary.

## Build Requirements

| Tool | Purpose |
|------|---------|
| Rust toolchain | Compile the binary (`cargo build --release`) |

That's it. The resulting binary is fully self-contained.
