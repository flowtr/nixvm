use anyhow::{Context, Result};
use nix_llvm::Compiler;
use rnix::Root;

fn main() -> Result<()> {
    let file_path = std::env::args().nth(1).context("no file path")?;
    let file = std::fs::read_to_string(&file_path).context("failed to read file")?;
    let parse = Root::parse(&file);

    for error in parse.errors() {
        eprintln!("error: {}", error);
    }

    if !parse.errors().is_empty() {
        return Err(anyhow::anyhow!("parse errors"));
    }

    let node = parse.tree().expr().context("no expression")?;
    println!("{:#?}", node);

    let mut compiler = Compiler::new()?;
    compiler.compile(&node)?;

    Ok(())
}
