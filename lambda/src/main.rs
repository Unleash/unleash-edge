use lambda_http::{run, service_fn, Body, Error, Request, RequestExt, Response};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

mod server;

use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use std::{sync::Arc, thread};

#[derive(Debug)]
struct ServerCell {}

static INSTANCE: OnceCell<ServerCell> = OnceCell::new();

/// This is the main body for the function.
/// Write your code inside it.
/// There are some code example in the following URLs:
/// - https://github.com/awslabs/aws-lambda-rust-runtime/tree/main/examples
async fn function_handler(event: Request) -> Result<Response<Body>, Error> {
    // Extract some useful information from the request
    let who = event
        .query_string_parameters_ref()
        .and_then(|params| params.first("name"))
        .unwrap_or("world");
    let message = format!("Hello {who}, this is an AWS Lambda HTTP request");

    if let None = INSTANCE.get() {
        println!("Nothing here making a new one");

        INSTANCE
            .set(ServerCell {})
            .expect("Failed to set lock marker");
        std::thread::spawn(|| {
            println!("Spawned server runtime thread");
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                println!("Spinning server");
                server::start_server().await.unwrap();
            });
        });
    } else {
        println!("Already have one");
    }
    // tokio::task::spawn(async move {
    //     server::start_server().await.unwrap();
    // });

    // Return something that implements IntoResponse.
    // It will be serialized to the right response event automatically by the runtime
    let resp = Response::builder()
        .status(200)
        .header("content-type", "text/html")
        .body(message.into())
        .map_err(Box::new)?;
    Ok(resp)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        // disable printing the name of the module in every log line.
        .with_target(false)
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        .init();

    println!("Pre weee");

    println!("Post weee");

    run(service_fn(function_handler)).await
}
