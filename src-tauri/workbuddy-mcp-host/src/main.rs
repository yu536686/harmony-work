use std::io::{self, BufRead, Write};
use workbuddy_mcp_host::{context_from_environment, handle_request};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let context = context_from_environment();
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: serde_json::Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("workbuddy-mcp-host invalid JSON-RPC input: {error}");
                continue;
            }
        };
        if let Some(response) = handle_request(&context, &request).await {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
