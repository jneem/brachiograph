use brachiologo::Env;
use clap::Parser;
use std::{path::PathBuf, process::exit};

#[derive(Parser)]
struct Args {
    input: PathBuf,
}

pub fn main() {
    let args = Args::parse();
    let input = match std::fs::read_to_string(&args.input) {
        Ok(x) => x,
        Err(e) => {
            println!(
                "Failed to open input file {}: {}",
                args.input.into_os_string().to_string_lossy(),
                e
            );
            exit(1);
        }
    };

    let (remaining, prog) = match brachiologo::parse::program(input.as_str().into()) {
        Ok(prog) => prog,
        Err(e) => {
            let (nom::Err::Failure(mut e) | nom::Err::Error(mut e)) = e else {
                panic!("unexpected error");
            };
            println!(
                "Parse error at {}:{}: {:?}",
                e.input.location_line(),
                e.input.get_utf8_column(),
                e.kind,
            );
            while let Some(cause) = e.cause {
                e = *cause;
                println!(
                    "Caused by (at {}:{}) {:?}",
                    e.input.location_line(),
                    e.input.get_utf8_column(),
                    e.kind,
                );
            }
            exit(1);
        }
    };

    // The parser returns an error if the input isn't consumed.
    assert!(remaining.is_empty());

    let mut env = Env::default();
    match prog.eval(&mut env) {
        Ok(None) => {}
        Ok(Some(e)) => {
            println!("Warning: program evaluated to an unexpected value: {}", e);
        }
        Err(e) => {
            println!("Evaluation error: {e}");
            exit(1);
        }
    }
}
