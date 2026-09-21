//! Generate a TypeScript contract from executed native or WASM output.

fn main() {
    if let Err(error) = run() {
        eprintln!("ipp-schema-gen: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: ipp-schema-gen INPUT.contract OUTPUT.ts".into());
    }
    let source = std::fs::read(&args[0])?;
    let output = ipp_schema_gen::generate(&source)?;
    if let Some(parent) = std::path::Path::new(&args[1]).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&args[1], output)?;
    Ok(())
}
