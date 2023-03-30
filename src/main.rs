use anyhow::{Context, Result};
use nix_llvm::Compiler;
use rnix::Root;

fn main() -> Result<()> {
    let parse = Root::parse("1 + 2");

    for error in parse.errors() {
        eprintln!("error: {}", error);
    }

    let node = parse.tree().expr().context("no expression")?;
    println!("{:#?}", node);

    let mut compiler = Compiler::new()?;
    compiler.compile(&node)?;

    Ok(())
}
