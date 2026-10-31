//! Android 15+ devices may use 16 KB memory pages, so every ELF LOAD segment must be
//! aligned to at least 16 KB. This link argument guarantees it regardless of NDK
//! version or how `RUSTFLAGS` are propagated by cargo-ndk. CI additionally verifies
//! the resulting binaries with `llvm-readelf`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-arg-cdylib=-Wl,-z,max-page-size=16384");
    }
}
