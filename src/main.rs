use anyhow::{Result, bail};

fn print_usage() {
    eprintln!("twk - Twinkle compiler (stage0 bootstrap)");
    eprintln!();
    eprintln!("Usage: twk <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  parse <file>                    Parse and display AST");
    eprintln!("  check <file>                    Type-check a source file");
    eprintln!("  lower <file>                    Lower to Core IR");
    eprintln!("  lower-anf <file>                Lower to ANF IR");
    eprintln!("  opt <file> [--show-original]    Optimize and display ANF IR");
    eprintln!("  build <file> [-o path] [--emit-wat]  Compile to WAT/Wasm");
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    let cmd = args[0].as_str();

    match cmd {
        "parse" => {
            let file = require_file(&args, cmd)?;
            twinkle::cli::parse::parse_file(&file)?;
        }
        "check" => {
            let file = require_file(&args, cmd)?;
            twinkle::cli::check::check_file(&file)?;
        }
        "lower" => {
            let file = require_file(&args, cmd)?;
            twinkle::cli::lower::lower_file(&file)?;
        }
        "lower-anf" => {
            let file = require_file(&args, cmd)?;
            let path = std::path::Path::new(&file);
            twinkle::cli::lower_anf::cmd_lower_anf(path)?;
        }
        "opt" => {
            let file = require_file(&args, cmd)?;
            let show_original = args[2..].contains(&"--show-original".to_string());
            let path = std::path::Path::new(&file);
            twinkle::cli::opt::cmd_opt(path, show_original)?;
        }
        "build" => {
            let file = require_file(&args, cmd)?;
            let mut output = None;
            let mut emit_wat = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "-o" | "--output" => {
                        i += 1;
                        if i >= args.len() {
                            bail!("-o requires a path argument");
                        }
                        output = Some(args[i].clone());
                    }
                    "--emit-wat" => emit_wat = true,
                    other => bail!("unknown option for build: {other}"),
                }
                i += 1;
            }
            twinkle::cli::build::build_file(&file, output.as_deref(), emit_wat)?;
        }
        "--help" | "-h" | "help" => {
            print_usage();
        }
        other => {
            eprintln!("unknown command: {other}");
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn require_file(args: &[String], cmd: &str) -> Result<String> {
    if args.len() < 2 {
        bail!("{cmd} requires a <file> argument");
    }
    Ok(args[1].clone())
}
