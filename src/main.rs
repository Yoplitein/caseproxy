#![allow(unused, non_snake_case)]

use std::{
    convert::Infallible,
    path::PathBuf,
};

use anyhow::{anyhow, Context};
use caseproxy::AResult;
use clap::Parser;
use hyper::{server::conn::http1, service::service_fn, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, UnixListener};

#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long, conflicts_with = "socketPath")]
    port: Option<i16>,

    #[arg(short = 'H', long, requires = "port", default_value = "localhost")]
    host: String,

    #[arg(short, long, conflicts_with = "port")]
    socketPath: Option<PathBuf>,

    #[arg(short, long, default_value = ".")]
    rootPath: PathBuf,

    #[arg(short, long, default_value = "/")]
    urlPrefix: String,
}

#[tokio::main]
async fn main() -> AResult<()> {
    let args = argfile::expand_args(argfile::parse_fromfile, argfile::PREFIX)?;
    let args = Args::try_parse_from(args)?;
    dbg!(&args);

    if matches!(
        args,
        Args {
            port: None,
            socketPath: None,
            ..
        }
    ) {
        return Err(anyhow!("One of --port or --socket-path must be given"));
    }

    macro_rules! main_loop {
        ($listener:ident) => {
            loop {
                let (client, clientAddr) = $listener.accept().await?;
                let io = TokioIo::new(client);
                tokio::task::spawn(async move {
                    let res = http1::Builder::new()
                        .serve_connection(io, service_fn(handle_request))
                        .await;
                    if let Err(err) = res {
                        eprintln!("Failed serving connection from {clientAddr:?}: {err:?}");
                    }
                });
            }
        };
    }

    if let Some(port) = args.port {
        let host = &format!("{}:{}", args.host, port);

        let mut candidateAddresses = tokio::net::lookup_host(host)
            .await
            .context(format!("invalid host address {host:?}"))?
            .collect::<Vec<_>>();
        if candidateAddresses.is_empty() {
            return Err(anyhow!(
                "lookup of hostname {host:?} yields zero addresses?!"
            ));
        }
        // prefer ipv4
        candidateAddresses.sort_by(|l, r| l.is_ipv6().cmp(&r.is_ipv6()));

        let mut listener = TcpListener::bind(candidateAddresses.first().unwrap()).await?;
        main_loop!(listener)
    } else if let Some(socketPath) = args.socketPath {
        let mut listener = UnixListener::bind(socketPath)?;
        main_loop!(listener)
    } else {
        unreachable!()
    }
}

async fn handle_request(
    req: Request<impl hyper::body::Body>,
) -> Result<Response<String>, Infallible> {
    let body = format!("requested {:?}", req.uri());
    Ok(Response::new(body))
}
