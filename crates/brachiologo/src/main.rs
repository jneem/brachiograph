use brachiologo::Env;
use clap::Parser;
use nom::error::{convert_error, VerboseError};
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
            let nom::Err::Failure(e) = e else {
                panic!("unexpected error");
            };
            let errors = e
                .errors
                .into_iter()
                .map(|(input, error)| (*input.fragment(), error))
                .collect();
            println!(
                "Parse error: {}",
                convert_error(input.as_str(), VerboseError { errors })
            );
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
