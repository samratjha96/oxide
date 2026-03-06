//! `oxide metrics` — Show metrics for a model.

pub fn execute(model_name: Option<&str>) -> anyhow::Result<()> {
    println!("⚡ Oxide — Metrics");
    println!("──────────────────");

    if let Some(name) = model_name {
        println!("  Model: {}", name);
        println!("  (Metrics available when running as a daemon with a loaded model)");
    } else {
        println!("  No model specified.");
        println!("  Usage: oxide metrics <model-name>");
    }

    println!("\n  Tip: Start the runtime with 'oxide run <model>' to collect metrics.");
    Ok(())
}
