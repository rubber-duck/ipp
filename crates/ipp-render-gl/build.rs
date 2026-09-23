//! Embed GLSL sources without comments or blank lines.
//!
//! The shader files under `src/services/render` keep their documentation; the
//! renderer embeds copies written here to `OUT_DIR`, which WebAssembly builds
//! would otherwise carry as data. Each line loses `//` and `/* */` comments and
//! surrounding whitespace, and empty lines are dropped, so preprocessor
//! directives, identifiers and code on a line are unchanged. A line comment that
//! holds only an upper-case placeholder, such as `// CUSTOM_DECLARATIONS`,
//! remains because shader composition replaces it.

use std::path::{Path, PathBuf};

/// Directories holding embedded shaders, relative to the manifest.
const SHADER_DIRECTORIES: [&str; 2] = ["src/services/render", "src/services/render/shaders"];

fn main() {
    let manifest =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-changed=build.rs");
    for directory in SHADER_DIRECTORIES {
        let source_directory = manifest.join(directory);
        println!("cargo:rerun-if-changed={}", source_directory.display());
        let relative = Path::new(directory)
            .strip_prefix("src/services/render")
            .expect("shader directories are under the render service");
        let destination = output.join("render").join(relative);
        std::fs::create_dir_all(&destination).expect("create shader output directory");
        let mut entries: Vec<_> = std::fs::read_dir(&source_directory)
            .expect("read shader directory")
            .map(|entry| entry.expect("read shader entry").path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("vert" | "frag" | "glsl")
                )
            })
            .collect();
        entries.sort();
        for path in entries {
            println!("cargo:rerun-if-changed={}", path.display());
            let source = std::fs::read_to_string(&path).expect("read shader source");
            let name = path.file_name().expect("shader file name");
            std::fs::write(destination.join(name), strip_comments(&source))
                .expect("write embedded shader");
        }
    }
}

/// Remove comments, surrounding whitespace and empty lines, keeping one line
/// per remaining source line and placeholder comments.
fn strip_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len() / 2);
    let mut in_block = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if !in_block && is_placeholder(trimmed) {
            output.push_str(trimmed);
            output.push('\n');
            continue;
        }

        let mut code = String::new();
        let mut rest = line;
        loop {
            if in_block {
                match rest.find("*/") {
                    Some(end) => {
                        rest = &rest[end + 2..];
                        in_block = false;
                        // A block comment separates tokens like whitespace.
                        code.push(' ');
                    }
                    None => break,
                }
            } else {
                let line_comment = rest.find("//");
                let block_comment = rest.find("/*");
                match (line_comment, block_comment) {
                    (Some(line), Some(block)) if block < line => {
                        code.push_str(&rest[..block]);
                        rest = &rest[block + 2..];
                        in_block = true;
                    }
                    (None, Some(block)) => {
                        code.push_str(&rest[..block]);
                        rest = &rest[block + 2..];
                        in_block = true;
                    }
                    (Some(line), _) => {
                        code.push_str(&rest[..line]);
                        break;
                    }
                    (None, None) => {
                        code.push_str(rest);
                        break;
                    }
                }
            }
        }

        let code = code.trim();
        if !code.is_empty() {
            output.push_str(code);
            output.push('\n');
        }
    }
    output
}

/// `// NAME` where NAME is upper-case letters, digits and underscores.
fn is_placeholder(line: &str) -> bool {
    line.strip_prefix("// ").is_some_and(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    })
}
