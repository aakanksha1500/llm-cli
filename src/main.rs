//! llm-cli: ask an LLM a question from the terminal.
//!
//!   llm-cli "Explain Kafka partitions in two sentences"
//!   cat error.log | llm-cli --system "Summarise this log"

mod client;
mod error;

use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

use clap::Parser;

use client::Client;
use error::AppError;

#[derive(Parser, Debug)]
#[command(version, about = "Ask an LLM a question from the terminal")]
struct Args {
    /// The prompt. If omitted, the prompt is read from stdin.
    prompt: Vec<String>,

    /// Optional system prompt
    #[arg(short, long)]
    system: Option<String>,

    #[arg(short, long, default_value = "claude-haiku-5-5")]
    model: String,

    #[arg(long, default_value_t = 1024)]
    max_tokens: u32,

    /// API key (falls back to the ANTHROPIC_API_KEY environment variable)
    #[arg(long, env = "ANTHROPIC_API_KEY", hide_env_values = true)]
    api_key: Option<String>,

    #[arg(long, default_value_t = 60)]
    timeout: u64,

    /// Retries for rate limits and server errors
    #[arg(long, default_value_t = 3)]
    retries: u32,

    /// Print model, stop reason and token usage to stderr
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    // All the fallible work happens in `run`. Here we only decide how to
    // report failure, so there is exactly one place errors are printed.
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

/// `args` is moved in: `run` owns it and it is freed when `run` returns.
fn run(args: Args) -> Result<(), AppError> {
    let prompt = read_prompt(&args.prompt)?;

    // `ok_or` turns Option<String> into Result<String, AppError>.
    let api_key = args.api_key.ok_or(AppError::MissingApiKey)?;

    let client = Client::new(api_key, args.timeout, args.retries)?;
    let completion = client.complete(
        &prompt,
        &args.model,
        args.max_tokens,
        args.system.as_deref(),
    )?;

    println!("{}", completion.text);

    if args.verbose {
        eprintln!(
            "[model: {} | stop: {} | tokens in/out: {}/{}]",
            completion.model,
            completion.stop_reason.as_deref().unwrap_or("unknown"),
            completion.usage.input_tokens,
            completion.usage.output_tokens,
        );
    }
    Ok(())
}

/// Use the arguments if given, otherwise read piped stdin.
/// Borrows a slice of Strings (`&[String]`) rather than taking the Vec.
fn read_prompt(words: &[String]) -> Result<String, AppError> {
    let mut prompt = words.join(" ");

    if prompt.trim().is_empty() && !io::stdin().is_terminal() {
        io::stdin().read_to_string(&mut prompt)?;
    }

    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(AppError::EmptyPrompt);
    }
    Ok(prompt)
}
