//! `oxide info` — Show model information.

use oxide_models::OnnxModel;
use std::path::Path;

pub fn execute(model_path: &str) -> anyhow::Result<()> {
    let path = Path::new(model_path);
    if !path.exists() {
        anyhow::bail!("Model file not found: {}", model_path);
    }

    let model = OnnxModel::load(path)?;
    let info = model.info();

    println!("oxide info {}", model_path);
    println!("  id:            {}", info.id);
    println!("  version:       {}", info.version);
    println!("  format:        {}", info.format);
    println!("  size:          {:.2} KB ({} bytes)", info.size_bytes as f64 / 1024.0, info.size_bytes);
    println!("  quantization:  {}", info.quantization);

    println!("  inputs:");
    for input in &info.inputs {
        println!("    {} {:?} ({})", input.name, input.shape, input.dtype);
    }

    println!("  outputs:");
    for output in &info.outputs {
        println!("    {} {:?} ({})", output.name, output.shape, output.dtype);
    }

    Ok(())
}
