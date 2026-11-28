//! build.rs — compiles the UNMODIFIED SQLite amalgamation (vendored
//! from libsqlite3-sys 0.30.1) as one translation unit with the tiny
//! Rust-facing wrapper layer. gnu11: GCC 16's C23 default rejects the
//! amalgamation's tentative-definition patterns.

fn main() {
    let mut build = cc::Build::new();
    build
        .file("csrc/sqlite3_ffi_unit.c")
        .include("csrc")
        .flag_if_supported("-std=gnu11")
        .flag_if_supported("-DSQLITE_THREADSAFE=1")
        .flag_if_supported("-DSQLITE_OMIT_LOAD_EXTENSION")
        .flag_if_supported("-O2")
        .warnings(false);
    build.compile("sqlite3_ffi");

    println!("cargo:rerun-if-changed=csrc/sqlite3_ffi_unit.c");
    println!("cargo:rerun-if-changed=csrc/sqlite3.h");
}
