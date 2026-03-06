//! # Oxide CLI
//!
//! Command-line interface for the Oxide edge AI runtime.
//! Deploy, monitor, and manage AI models on edge devices.

mod commands;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "oxide",
    about = "Edge AI runtime — deploy ONNX models to resource-constrained devices",
    long_about = "Lightweight, secure edge AI runtime for deploying models to resource-constrained devices.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run inference on a model locally
    Run {
        /// Path to the model file (ONNX format)
        model: String,

        /// Input data as JSON array of f32 values
        #[arg(long)]
        input: Option<String>,

        /// Input tensor shape (e.g., "1,3,224,224")
        #[arg(long)]
        shape: Option<String>,

        /// Number of inference iterations for benchmarking
        #[arg(long, default_value = "1")]
        iterations: usize,
    },

    /// Deploy a model to a device or fleet
    Deploy {
        /// Path to the model file
        model: String,

        /// Target device ID
        #[arg(long)]
        device: Option<String>,

        /// Target fleet ID
        #[arg(long)]
        fleet: Option<String>,

        /// Rollout strategy: all_at_once, canary, rolling
        #[arg(long, default_value = "all_at_once")]
        rollout: String,
    },

    /// Manage devices
    Device {
        #[command(subcommand)]
        action: DeviceAction,
    },

    /// Manage fleets
    Fleet {
        #[command(subcommand)]
        action: FleetAction,
    },

    /// Show model information
    Info {
        /// Path to the model file
        model: String,
    },

    /// Encrypt a model file
    Encrypt {
        /// Path to the model file
        model: String,

        /// Output path for encrypted model
        #[arg(long)]
        output: Option<String>,

        /// Path to encryption key file (generated if not exists)
        #[arg(long, default_value = "oxide.key")]
        key: String,
    },

    /// Decrypt a model file
    Decrypt {
        /// Path to the encrypted model file
        model: String,

        /// Output path for decrypted model
        #[arg(long)]
        output: Option<String>,

        /// Path to encryption key file
        #[arg(long, default_value = "oxide.key")]
        key: String,
    },

    /// Start the control plane server
    Serve {
        /// Listen address
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        /// Listen port
        #[arg(long, default_value = "8080")]
        port: u16,
    },

    /// Show metrics for a running model
    Metrics {
        /// Model name
        model: Option<String>,
    },

    /// Run benchmarks on a model
    Bench {
        /// Path to the model file
        model: String,

        /// Number of warmup iterations
        #[arg(long, default_value = "10")]
        warmup: usize,

        /// Number of benchmark iterations
        #[arg(long, default_value = "100")]
        iterations: usize,

        /// Input tensor shape (e.g., "1,3,224,224")
        #[arg(long)]
        shape: Option<String>,
    },
}

#[derive(Subcommand)]
enum DeviceAction {
    /// List registered devices
    List,
    /// Register a new device
    Register {
        /// Device ID
        id: String,
        /// Device name
        #[arg(long)]
        name: String,
    },
    /// Show device status
    Status {
        /// Device ID
        id: String,
    },
}

#[derive(Subcommand)]
enum FleetAction {
    /// List all fleets
    List,
    /// Create a new fleet
    Create {
        /// Fleet ID
        id: String,
        /// Fleet name
        #[arg(long)]
        name: String,
    },
    /// Show fleet status
    Status {
        /// Fleet ID
        id: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    match cli.command {
        Commands::Run {
            model,
            input,
            shape,
            iterations,
        } => commands::run::execute(&model, input.as_deref(), shape.as_deref(), iterations)?,

        Commands::Deploy {
            model,
            device,
            fleet,
            rollout,
        } => commands::deploy::execute(&model, device.as_deref(), fleet.as_deref(), &rollout)?,

        Commands::Device { action } => match action {
            DeviceAction::List => commands::device::list()?,
            DeviceAction::Register { id, name } => commands::device::register(&id, &name)?,
            DeviceAction::Status { id } => commands::device::status(&id)?,
        },

        Commands::Fleet { action } => match action {
            FleetAction::List => commands::fleet::list()?,
            FleetAction::Create { id, name } => commands::fleet::create(&id, &name)?,
            FleetAction::Status { id } => commands::fleet::status(&id)?,
        },

        Commands::Info { model } => commands::info::execute(&model)?,

        Commands::Encrypt { model, output, key } => {
            commands::encrypt::execute(&model, output.as_deref(), &key)?
        }

        Commands::Decrypt { model, output, key } => {
            commands::decrypt::execute(&model, output.as_deref(), &key)?
        }

        Commands::Serve { host, port } => commands::serve::execute(&host, port).await?,

        Commands::Metrics { model } => commands::metrics::execute(model.as_deref())?,

        Commands::Bench {
            model,
            warmup,
            iterations,
            shape,
        } => commands::bench::execute(&model, warmup, iterations, shape.as_deref())?,
    }

    Ok(())
}
