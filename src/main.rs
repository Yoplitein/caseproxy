#![allow(unused, non_snake_case, non_upper_case_globals)]

use std::{
    cell::OnceCell, convert::Infallible, path::{Path, PathBuf}, sync::OnceLock
};

use anyhow::{anyhow, Context};
use caseproxy::{resolve_parents, AResult, InsensitivePath};
use clap::Parser;
use hyper::{server::conn::http1, service::service_fn, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, UnixListener};

#[derive(Debug, Parser)]
struct Config {
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

static serverConfig: OnceLock<Config> = OnceLock::new();

#[tokio::main]
async fn main() -> AResult<()> {
    let expanded = argfile::expand_args(argfile::parse_fromfile, argfile::PREFIX)?;
    let mut config = Config::try_parse_from(expanded)?;
    
    if !config.urlPrefix.starts_with("/") {
        config.urlPrefix.insert(0, '/');
    }
    if !config.urlPrefix.ends_with("/") {
        config.urlPrefix.push('/');
    }
    
    serverConfig.set(config).unwrap();
    let config = serverConfig.get().unwrap();
    dbg!(config);

    if matches!(
        config,
        Config {
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

    if let Some(port) = config.port {
        let host = &format!("{}:{}", config.host, port);

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
    } else if let Some(socketPath) = &config.socketPath {
        let mut listener = UnixListener::bind(socketPath)?;
        main_loop!(listener)
    } else {
        unreachable!()
    }
}

async fn handle_request(
    req: Request<impl hyper::body::Body>,
) -> AResult<Response<String>> {
    let config = serverConfig.get().unwrap();
    
    let reqPath = Path::new(
        req.uri().path()
    ).strip_prefix(&config.urlPrefix)?;
    let fullPath = resolve_parents(
        &config.rootPath.join(reqPath)
    );
    let file = resolve_path(InsensitivePath(fullPath.clone())).await;
    match file {
        Ok(file) => {
            // this check is not strictly necessary as it is sufficiently handled by prefix stripping in `find_matching_files`, but just in case that ever changes
            if !file.starts_with(&config.rootPath) {
                let mut res = Response::new(String::new());
                *res.status_mut() = StatusCode::FORBIDDEN;
                return Ok(res);
            }
            
            let body = format!("requested {:?}\ngot {:?}\n", fullPath, file);
            Ok(Response::new(body))
        },
        Err(err) => {
            let body = format!("requested {:?} but 404\n\nerr:\n{err:#?}\n{:#?}\n", fullPath, err.backtrace());
            let mut res = Response::new(body);
            *res.status_mut() = StatusCode::NOT_FOUND;
            Ok(res)
        },
    }
}

async fn resolve_path(path: InsensitivePath) -> AResult<PathBuf> {
    let config = serverConfig.get().unwrap();
    let files = tokio::task::spawn_blocking(move ||
        path.find_matching_files(Some(&config.rootPath))
    ).await??;
    // TODO: other strategies
    // TODO: caching
    Ok(files.into_iter().next().ok_or_else(|| anyhow!("not found"))?)
}
