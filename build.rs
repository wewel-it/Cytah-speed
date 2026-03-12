// build.rs executed before compilation. It ensures the RocksDB build finds C++ headers
// and links against the C++ standard library on Linux environments.

fn main() {
    // instruct rustc/linker to link the C++ standard library (stdc++)
    println!("cargo:rustc-link-lib=stdc++");

    // make sure the compiler sees the system C++ headers (specific path may
    // vary by distribution; adjust if necessary). This helps bindgen when
    // parsing rocksdb headers, avoiding the "cstdint file not found" error.
    let include_path = "/usr/include/c++/13";

    // export environment variables that build scripts and bindgen honor
    println!("cargo:rustc-env=CPATH={}", include_path);
    println!("cargo:rustc-env=CPLUS_INCLUDE_PATH={}", include_path);

    // instruct bindgen (used by librocksdb-sys) to pass an extra clang include
    // argument so it knows where to find the C++ headers.
    println!("cargo:rustc-env=BINDGEN_EXTRA_CLANG_ARGS=-I{}", include_path);
}
