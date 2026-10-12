use agent_history_manager::parsers;
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 3,
        "usage: parse_history codex|claude_code PATH"
    );
    let p = parsers::parse(std::path::Path::new(&args[2]), &args[1])?;
    println!(
        "{}",
        serde_json::json!({"session":p.session,"messages":p.messages})
    );
    Ok(())
}
