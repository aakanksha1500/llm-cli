# llm-cli

A small command-line tool, written in Rust, that sends a prompt to the Anthropic Messages API and prints the reply. Built to learn Rust's ownership model, error handling and dependency management on a real HTTP and JSON workload.

```bash
llm-cli "Explain Kafka consumer groups in two sentences"
cat airflow_task.log | llm-cli --system "Find the root cause of this failure"
```

## Features

- Prompt from arguments or piped stdin, so it composes with other shell tools
- Typed request and response structs, serialised and parsed with `serde`
- One error enum (`thiserror`) covering config, network, HTTP status and parse failures
- Retries with exponential backoff on 429 and 5xx responses only; auth and validation errors fail fast
- Distinct exit codes per failure class, so scripts can branch on them
- Unit tests for JSON parsing and error classification that run without network access
- CI on every push: format check, clippy with warnings as errors, tests

## Install

```bash
git clone https://github.com/<your-username>/llm-cli
cd llm-cli
cargo install --path .
export ANTHROPIC_API_KEY=sk-ant-...
```

## Usage

```
llm-cli [OPTIONS] [PROMPT]...

  -s, --system <SYSTEM>          Optional system prompt
  -m, --model <MODEL>            [default: claude-haiku-5-5]
      --max-tokens <MAX_TOKENS>  [default: 1024]
      --api-key <API_KEY>        Falls back to ANTHROPIC_API_KEY
      --timeout <TIMEOUT>        Seconds [default: 60]
      --retries <RETRIES>        Retries for 429/5xx [default: 3]
  -v, --verbose                  Print model, stop reason and token usage to stderr
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 2 | Bad input: missing API key or empty prompt |
| 3 | API returned an error status |
| 4 | Network failure |
| 5 | Response could not be parsed |
| 6 | Could not read stdin |

## Design notes

**Ownership.** `Client::new` takes the API key by value, so the client owns it. `complete` borrows the prompt and model as `&str`, which means the caller keeps them and nothing is copied. The request struct borrows its strings with a lifetime (`Request<'a>`) instead of cloning them into owned `String`s.

**Errors.** Every fallible function returns `Result<T, AppError>`. The `?` operator plus `#[from]` attributes convert library errors (`reqwest`, `serde_json`, `std::io`) into the right variant without manual mapping. Errors are printed in exactly one place, in `main`.

**Parsing.** Response content blocks use a serde tagged enum. Unknown block types fall into an `Other` variant rather than failing the whole response, so the tool keeps working if the API adds new block types.

**Testability.** HTTP and parsing are separated. `parse_completion` and `api_error` take a string and return a result, so they are tested directly with fixture JSON.

## Project layout

```
src/
  main.rs    CLI arguments, stdin handling, exit codes
  client.rs  HTTP client, request/response types, retry loop
  error.rs   AppError enum, retry and exit code rules
```

## Development

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```
