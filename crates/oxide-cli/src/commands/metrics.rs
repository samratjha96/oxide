//! `oxide metrics` — Show metrics for a model.

pub fn execute(model_name: Option<&str>) -> anyhow::Result<()> {
    if let Some(name) = model_name {
        println!("oxide metrics {}", name);
        println!("  (metrics available when running as a daemon with a loaded model)");
    } else {
        println!("oxide metrics");
        println!("  no model specified");
        println!("  usage: oxide metrics <model-name>");
    }

    println!("\n  tip: start the runtime with 'oxide run <model>' to collect metrics.");
    Ok(())
}
