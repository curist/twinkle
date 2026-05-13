use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "twk")]
#[command(about = "Twinkle compiler - A statically typed language targeting WebAssembly GC", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse a Twinkle source file and display the AST
    Parse {
        /// Path to the .tw file
        file: String,
    },
    /// Type-check a Twinkle source file
    Check {
        /// Path to the .tw file
        file: String,
    },
    /// Lower a Twinkle source file to Core IR and display it
    Lower {
        /// Path to the .tw file
        file: String,
    },
    /// Lower a Twinkle source file to ANF IR and display it
    LowerAnf {
        /// Path to the .tw file
        file: String,
    },
    /// Optimise a Twinkle source file and display the resulting ANF IR
    Opt {
        /// Path to the .tw file
        file: String,
        /// Also print the unoptimized ANF before the optimized form
        #[arg(long)]
        show_original: bool,
    },
    /// Run a Twinkle program
    Run {
        /// Path to the .tw file
        file: String,
        /// Arguments passed to the Twinkle program (must come after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile a Twinkle program to WAT/Wasm
    Build {
        /// Path to the .tw file
        file: String,
        /// Output file path
        #[arg(short, long)]
        output: Option<String>,
        /// Also emit a sibling .wat file when outputting .wasm
        #[arg(long)]
        emit_wat: bool,
    },
    /// Dump the built-in runtime as linked WAT
    RuntimeDump,
    /// Run the language server over stdio
    Lsp,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Parse { file } => {
            twinkle::cli::parse::parse_file(&file)?;
        }
        Commands::Check { file } => {
            twinkle::cli::check::check_file(&file)?;
        }
        Commands::Lower { file } => {
            twinkle::cli::lower::lower_file(&file)?;
        }
        Commands::LowerAnf { file } => {
            let path = std::path::Path::new(&file);
            twinkle::cli::lower_anf::cmd_lower_anf(path)?;
        }
        Commands::Opt {
            file,
            show_original,
        } => {
            let path = std::path::Path::new(&file);
            twinkle::cli::opt::cmd_opt(path, show_original)?;
        }
        Commands::Run { file, args } => {
            twinkle::cli::run_wasm::run_wasm_file_with_args(&file, &args)?;
        }
        Commands::Build {
            file,
            output,
            emit_wat,
        } => {
            twinkle::cli::build::build_file(&file, output.as_deref(), emit_wat)?;
        }
        Commands::RuntimeDump => {
            twinkle::cli::runtime_dump::runtime_dump()?;
        }
        Commands::Lsp => {
            twinkle::cli::lsp::cmd_lsp()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_subcommand_parses_file_and_args() {
        let cli = Cli::try_parse_from(["twk", "run", "tests/run/hello.tw"]).expect("parse args");
        match cli.command {
            Commands::Run { file, args } => {
                assert_eq!(file, "tests/run/hello.tw");
                assert!(args.is_empty(), "run should default to no passthrough args");
            }
            _ => panic!("expected run subcommand"),
        }
    }

    #[test]
    fn run_subcommand_accepts_passthrough_args_after_double_dash() {
        let cli = Cli::try_parse_from([
            "twk",
            "run",
            "boot/main.tw",
            "--",
            "run",
            "foo.tw",
            "--emit-wat",
        ])
        .expect("parse args");
        match cli.command {
            Commands::Run { file, args } => {
                assert_eq!(file, "boot/main.tw");
                assert_eq!(args, vec!["run", "foo.tw", "--emit-wat"]);
            }
            _ => panic!("expected run subcommand"),
        }
    }

    #[test]
    fn unknown_runtime_subcommand_is_rejected() {
        let parsed = Cli::try_parse_from(["twk", "runwasm", "tests/run/hello.tw"]);
        match parsed {
            Ok(_) => panic!("unknown subcommand should be rejected"),
            Err(err) => {
                let rendered = err.to_string();
                assert!(
                    rendered.contains("unrecognized subcommand")
                        || rendered.contains("unknown subcommand"),
                    "unexpected clap error: {rendered}"
                );
            }
        }
    }
}
