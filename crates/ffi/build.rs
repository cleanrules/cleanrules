//! The shared library's name for the dynamic loader, so that it works once
//! installed, not only where it was built: an `@rpath` install name on Apple
//! targets (otherwise the absolute path of the build), and a SONAME on ELF
//! targets (otherwise none). The target's, not the host's.

use std::env;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let vendor = env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
    let family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    if vendor == "apple" {
        println!("cargo::rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libcleanrules_ffi.dylib");
    } else if family.split(',').any(|f| f == "unix") {
        println!("cargo::rustc-cdylib-link-arg=-Wl,-soname,libcleanrules_ffi.so");
    }
}
