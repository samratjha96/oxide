//! `oxide run` — Run inference on a model locally.

use oxide_runtime::InferenceEngine;
use std::path::Path;
use std::time::Instant;

pub fn execute(
    model_path: &str,
    input_json: Option<&str>,
    shape_str: Option<&str>,
    iterations: usize,
) -> anyhow::Result<()> {
    let path = Path::new(model_path);
    if !path.exists() {
        anyhow::bail!("Model file not found: {}", model_path);
    }

    println!("⚡ Oxide — Loading model: {}", model_path);
    let engine = InferenceEngine::new(0);

    let start = Instant::now();
    let info = engine.load_model(path)?;
    let load_time = start.elapsed();

    println!("✓ Model loaded in {:.2?}", load_time);
    println!("  ID:      {}", info.id);
    println!("  Format:  {}", info.format);
    println!("  Size:    {:.2} KB", info.size_bytes as f64 / 1024.0);
    println!("  Inputs:  {:?}", info.inputs.iter().map(|i| format!("{}:{:?}", i.name, i.shape)).collect::<Vec<_>>());
    println!("  Outputs: {:?}", info.outputs.iter().map(|o| format!("{}:{:?}", o.name, o.shape)).collect::<Vec<_>>());

    // Parse input data
    let (input_data, input_shape) = if let Some(json) = input_json {
        let data: Vec<f32> = serde_json::from_str(json)
            .map_err(|e| anyhow::anyhow!("Invalid input JSON: {}", e))?;
        let shape = if let Some(s) = shape_str {
            parse_shape(s)?
        } else {
            vec![data.len()]
        };
        (data, shape)
    } else {
        // Auto-generate input based on model's expected input shape
        let shapes = engine.get_model_info(&info.id)?.inputs;
        if shapes.is_empty() {
            anyhow::bail!("Model has no inputs defined. Provide --input and --shape.");
        }
        let shape: Vec<usize> = shapes[0]
            .shape
            .iter()
            .map(|&d| if d < 0 { 1 } else { d as usize })
            .collect();
        let size: usize = shape.iter().product();
        let data = vec![0.0f32; size];
        println!("  Using zero input with shape {:?}", shape);
        (data, shape)
    };

    // Run inference
    println!("\n🔥 Running {} inference iteration(s)...", iterations);
    let mut total_time = std::time::Duration::ZERO;

    for i in 0..iterations {
        let start = Instant::now();
        let result = engine.infer(&info.id, &input_data, &input_shape)?;
        let elapsed = start.elapsed();
        total_time += elapsed;

        if iterations == 1 || i == iterations - 1 {
            println!(
                "  Iteration {}: {:.2?} ({:.2}us)",
                i + 1,
                elapsed,
                elapsed.as_secs_f64() * 1_000_000.0
            );
            if result.outputs.len() <= 20 {
                println!("  Output: {:?}", result.outputs);
            } else {
                println!(
                    "  Output: [{:.4}, {:.4}, ... {} values total]",
                    result.outputs[0],
                    result.outputs[1],
                    result.outputs.len()
                );
            }
        }
    }

    if iterations > 1 {
        let avg = total_time / iterations as u32;
        let metrics = engine.get_metrics(&info.id)?;
        println!("\n📊 Benchmark Results ({} iterations):", iterations);
        println!("  Total time: {:.2?}", total_time);
        println!("  Avg time:   {:.2?}", avg);
        println!("  P50:        {:.2}us", metrics.p50_latency_us);
        println!("  P95:        {:.2}us", metrics.p95_latency_us);
        println!("  P99:        {:.2}us", metrics.p99_latency_us);
        println!("  Throughput: {:.1} inferences/sec", metrics.throughput_per_sec);
    }

    println!("\n✅ Done.");
    Ok(())
}

fn parse_shape(s: &str) -> anyhow::Result<Vec<usize>> {
    s.split(',')
        .map(|dim| {
            dim.trim()
                .parse::<usize>()
                .map_err(|e| anyhow::anyhow!("Invalid shape dimension '{}': {}", dim, e))
        })
        .collect()
}
