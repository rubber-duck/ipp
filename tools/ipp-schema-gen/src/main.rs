//! Generate a TypeScript contract and its test/tool wire manifest from executed native or WASM output.

fn main() {
    if let Err(error) = run() {
        eprintln!("ipp-schema-gen: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: ipp-schema-gen INPUT.contract OUTPUT.ts (also writes OUTPUT-manifest.ts)"
                .into(),
        );
    }
    let source = std::fs::read(&args[0])?;
    let generated = ipp_schema_gen::generate(&source)?;

    let client = std::path::Path::new(&args[1]);
    let stem = client
        .file_stem()
        .ok_or("output path has no file name")?
        .to_string_lossy();
    // Tests and tools import the descriptive manifest from `OUTPUT-manifest.ts`.
    let manifest = client.with_file_name(format!("{stem}-manifest.ts"));
    if let Some(parent) = client.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(client, generated.client)?;
    std::fs::write(manifest, generated.manifest)?;
    Ok(())
}
