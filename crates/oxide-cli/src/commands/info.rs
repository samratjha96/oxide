//! `oxide info` — Show model information.

use oxide_models::OnnxModel;
use std::path::Path;

pub fn execute(model_path: &str) -> anyhow::Result<()> {
    let path = Path::new(model_path);
    if !path.exists() {
        anyhow::bail!("Model file not found: {}", model_path);
    }

    println!("⚡ Oxide — Model Info");
    println!("─────────────────────");

    let model = OnnxModel::load(path)?;
    let info = model.info();

    println!("  File:          {}", model_path);
    println!("  ID:            {}", info.id);
    println!("  Version:       {}", info.version);
    println!("  Format:        {}", info.format);
    println!("  Size:          {:.2} KB ({} bytes)", info.size_bytes as f64 / 1024.0, info.size_bytes);
    println!("  Quantization:  {}", info.quantization);

    println!("\n  Inputs:");
    for input in &info.inputs {
        println!("    - {} {:?} ({})", input.name, input.shape, input.dtype);
    }

    println!("\n  Outputs:");
    for output in &info.outputs {
        println!("    - {} {:?} ({})", output.name, output.shape, output.dtype);
    }

    Ok(())
}
