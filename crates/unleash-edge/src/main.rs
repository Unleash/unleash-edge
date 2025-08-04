use clap::Parser;
use unleash_edge_cli::{CliArgs, EdgeMode};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = unleash_edge_cli::CliArgs::parse();
    if args.markdown_help {
        clap_markdown::print_help_markdown::<CliArgs>();
        return Ok(());
    }

    match args.mode {
        EdgeMode::Health(health_args) => health_checker::check_health(health_args).await,
        EdgeMode::Ready(ready_args) => ready_checker::check_ready(ready_args).await,
        _ => run_server(args).await,
    }
    .map_err(|e| e.into())
}

async fn run_server(args: CliArgs) -> EdgeResult<()> {}
