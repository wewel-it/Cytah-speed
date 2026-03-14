// build.rs executed before compilation. It ensures the RocksDB build finds C++ headers
// and links against the C++ standard library on Linux environments.

use std::env;
use std::path::PathBuf;

fn main() {
    // Instruct rustc/linker to link against the C++ standard library (stdc++)
    println!("cargo:rustc-link-lib=stdc++");

    // Make sure the compiler finds the system C++ headers.
    // The path below matches Ubuntu 24 default location; adjust if needed.
    let include_path = "/usr/include/c++/13";

    // Export environment variables for build scripts (librocksdb-sys / bindgen).
    println!("cargo:rustc-env=CPATH={}", include_path);
    println!("cargo:rustc-env=CPLUS_INCLUDE_PATH={}", include_path);
    println!("cargo:rustc-env=BINDGEN_EXTRA_CLANG_ARGS=-I{}", include_path);

    // Ensure the C++ compiler defines the <cstdint> types when compiling RocksDB.
    // Some RocksDB versions omit <cstdint> in headers, which breaks modern Clang/GCC.
    // We set CXXFLAGS to include <cstdint> automatically.
    println!("cargo:rustc-env=CXXFLAGS=-include<cstdint>");

    // Additionally, patch RocksDB headers in the cargo registry (if present) to
    // include <cstdint> directly to avoid compilation errors.
    if let Ok(cargo_home) = env::var("CARGO_HOME") {
        patch_rocksdb_headers_in_registry(&cargo_home);
    } else if let Ok(home) = env::var("HOME") {
        let cargo_home = PathBuf::from(home).join(".cargo");
        patch_rocksdb_headers_in_registry(&cargo_home.to_string_lossy());
    }
}

fn patch_rocksdb_headers_in_registry(cargo_home: &str) {
    use std::fs;
    use std::io::Write;

    let registry_src = PathBuf::from(cargo_home).join("registry/src");
    if !registry_src.exists() {
        return;
    }

    // Find rocksdb crate directories inside registry/src
    let entries = match fs::read_dir(&registry_src) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Look for data_block_hash_index.h in this directory tree
        let mut stack = vec![path];
        while let Some(dir) = stack.pop() {
            if let Ok(read_dir) = fs::read_dir(&dir) {
                for child in read_dir.flatten() {
                    let child_path = child.path();
                    if child_path.is_dir() {
                        stack.push(child_path);
                        continue;
                    }

                    if let Some(name) = child_path.file_name().and_then(|n| n.to_str()) {
                        if name == "data_block_hash_index.h" || name == "string_util.h" {
                            // Insert #include <cstdint> if missing
                            if let Ok(contents) = fs::read_to_string(&child_path) {
                                if !contents.contains("#include <cstdint>") {
                                    if let Ok(mut file) = fs::OpenOptions::new()
                                        .write(true)
                                        .truncate(true)
                                        .open(&child_path)
                                    {
                                        // Prepend the include to the file
                                        let new_contents = format!("#include <cstdint>\n{}", contents);
                                        let _ = file.write_all(new_contents.as_bytes());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
