//! `oxide bench` — Benchmark a model.

use oxide_runtime::InferenceEngine;
use std::path::Path;
use std::time::Instant;

pub fn execute(
    model_path: &str,
    warmup: usize,
    iterations: usize,
    shape_str: Option<&str>,
) -> anyhow::Result<()> {
    let path = Path::new(model_path);
    if !path.exists() {
        anyhow::bail!("Model file not found: {}", model_path);
    }

    println!("oxide bench {}", model_path);

    let engine = InferenceEngine::new(0);

    // Load model and time it
    let load_start = Instant::now();
    let info = engine.load_model(path)?;
    let load_time = load_start.elapsed();

    println!("  model:      {}", info.id);
    println!("  format:     {}", info.format);
    println!("  size:       {:.2} KB", info.size_bytes as f64 / 1024.0);
    println!("  load time:  {:.2?}", load_time);
    println!("  threads:    {}", engine.num_threads());

    // Determine input shape
    let input_shape: Vec<usize> = if let Some(s) = shape_str {
        s.split(',')
            .map(|d| d.trim().parse::<usize>())
            .collect::<Result<_, _>>()?
    } else if !info.inputs.is_empty() {
        info.inputs[0]
            .shape
            .iter()
            .map(|&d| if d < 0 { 1 } else { d as usize })
            .collect()
    } else {
        anyhow::bail!("Cannot determine input shape. Use --shape flag.");
    };

    let input_size: usize = input_shape.iter().product();
    let input_data = vec![0.0f32; input_size];
    println!("  input:      {:?} ({} elements)", input_shape, input_size);

    // Warmup
    println!("\n  warmup ({} iterations)...", warmup);
    for _ in 0..warmup {
        engine.infer(&info.id, &input_data, &input_shape)?;
    }

    // Benchmark
    println!("  benchmarking ({} iterations)...", iterations);
    let bench_start = Instant::now();
    for _ in 0..iterations {
        engine.infer(&info.id, &input_data, &input_shape)?;
    }
    let bench_time = bench_start.elapsed();

    let metrics = engine.get_metrics(&info.id)?;

    println!("\n  results");
    println!("  ────────────────────────────────");
    println!("  total time:    {:.2?}", bench_time);
    println!(
        "  avg latency:   {:.2}us ({:.2}ms)",
        metrics.avg_latency_us,
        metrics.avg_latency_us / 1000.0
    );
    println!(
        "  p50 latency:   {:.2}us ({:.2}ms)",
        metrics.p50_latency_us,
        metrics.p50_latency_us / 1000.0
    );
    println!(
        "  p95 latency:   {:.2}us ({:.2}ms)",
        metrics.p95_latency_us,
        metrics.p95_latency_us / 1000.0
    );
    println!(
        "  p99 latency:   {:.2}us ({:.2}ms)",
        metrics.p99_latency_us,
        metrics.p99_latency_us / 1000.0
    );
    println!(
        "  max latency:   {:.2}us ({:.2}ms)",
        metrics.max_latency_us,
        metrics.max_latency_us / 1000.0
    );
    println!("  throughput:    {:.1} inferences/sec", metrics.throughput_per_sec);
    println!(
        "  total infers:  {} ({} failed)",
        metrics.total_inferences, metrics.failed_inferences
    );
    println!("  ────────────────────────────────");

    // Performance assessment
    if metrics.avg_latency_us < 10_000.0 {
        println!("\n  avg latency < 10ms — excellent");
    } else if metrics.avg_latency_us < 50_000.0 {
        println!("\n  avg latency < 50ms — good");
    } else {
        println!("\n  avg latency > 50ms — consider quantization");
    }

    Ok(())
}
