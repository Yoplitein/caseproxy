#![allow(unused, non_snake_case, non_upper_case_globals)]

use std::{
    cell::OnceCell, convert::Infallible, path::{Path, PathBuf}, sync::OnceLock
};

use anyhow::{anyhow, Context};
use caseproxy::{resolve_parents, AResult, InsensitivePath};
use clap::Parser;
use futures_util::TryStreamExt;
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use hyper::{body::{Bytes, Frame}, header::HeaderValue, server::conn::http1, service::service_fn, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, UnixListener};
use tokio_util::io::ReaderStream;

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
    
    #[arg(long, conflicts_with = "nginxUrl")]
    sendfile: bool,
    
    #[arg(long = "nginx", conflicts_with = "sendfile")]
    nginxUrl: Option<String>,
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
    
    if let Some(url) = &mut config.nginxUrl {
        if !url.starts_with("/") {
            url.insert(0, '/');
        }
        if !url.ends_with("/") {
            url.push('/');
        }
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

type ABody = BoxBody<Bytes, anyhow::Error>;

async fn handle_request(
    req: Request<impl hyper::body::Body>,
) -> AResult<Response<ABody>> {
    let config = serverConfig.get().unwrap();
    
    let reqPath = Path::new(
        req.uri().path()
    ).strip_prefix(&config.urlPrefix)?;
    let fullPath = resolve_parents(
        &config.rootPath.join(reqPath)
    );
    let file = resolve_path(InsensitivePath(fullPath.clone())).await;
    match file {
        Err(err) => {
            Ok(status_response(StatusCode::NOT_FOUND))
        },
        Ok(file) => {
            // this check is technically unnecessary as it is sufficiently handled by prefix stripping in `find_matching_files`, but just in case that ever changes
            if !file.starts_with(&config.rootPath) {
                return Ok(status_response(StatusCode::FORBIDDEN));
            }
            
            if config.sendfile {
                let file = file.canonicalize()?;
                let body = Bytes::new();
                let body = Full::new(body).map_err(|e| match e {}).boxed();
                let response = Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .header("X-Sendfile", HeaderValue::from_bytes(file.as_os_str().as_encoded_bytes())?)
                    .body(body)?;
                Ok(response)
            } else if let Some(nginxUrl) = &config.nginxUrl {
                let file = file.strip_prefix(&config.rootPath)?;
                let body = Bytes::new();
                let body = Full::new(body).map_err(|e| match e {}).boxed();
                let mut fullUrl = Vec::new();
                fullUrl.extend(nginxUrl.as_bytes());
                fullUrl.extend(file.as_os_str().as_encoded_bytes());
                let response = Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .header("X-Accel-Redirect", HeaderValue::from_bytes(&fullUrl)?)
                    .body(body)?;
                Ok(response)
            } else {
                let file = tokio::fs::File::open(file).await?;
                let length = file.metadata().await?.len();
                let fileStream = ReaderStream::new(file).map_ok(Frame::data);
                let body = StreamBody::new(fileStream);
                let body = BodyExt::map_err(body, |e| anyhow!(e)).boxed();
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Length", format!("{length}"))
                    .body(body)?;
                Ok(response)
            }
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

fn status_response(code: StatusCode) -> Response<ABody> {
    let message = code.canonical_reason().unwrap_or("unknown");
    let body = Bytes::from_static(message.as_bytes());
    let body = Full::new(body).map_err(|e| match e {}).boxed();
    let mut res = Response::new(body);
    *res.status_mut() = code;
    res
}
