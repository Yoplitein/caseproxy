#![allow(non_snake_case)]

use std::{
    collections::{HashMap, VecDeque},
    fmt::Write,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use caseproxy::{AResult, InsensitivePath};
use clap::Parser;
use sha3::Digest;

#[derive(Debug, Parser)]
struct Args {
    rootDir: PathBuf,

    /// Path to save an HTML report to
    #[arg(long)]
    html: Option<PathBuf>,
}

fn main() -> AResult<()> {
    let args = Args::parse();

    let files = find_all_files(&args.rootDir)?;
    let mut files: Vec<_> = files.into_iter().map(InsensitivePath).collect();
    files.sort();

    let mut duplicateSets: HashMap<InsensitivePath, Vec<PathBuf>> = HashMap::new();
    for file in files {
        duplicateSets
            .entry(file.clone())
            .and_modify(|v| v.push(file.0.clone()))
            .or_insert_with(|| vec![file.0]);
    }
    duplicateSets.retain(|_, v| v.len() > 1);

    let mut fileHashes = HashMap::new();
    for file in duplicateSets.values().flat_map(std::convert::identity) {
        let hash = match hash_file(file) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("couldn't read {file:?} for hashing: {err:?}");
                fileHashes.insert(file.to_path_buf(), "error".to_string());
                continue;
            }
        };
        fileHashes.insert(file.to_path_buf(), hash);
    }

    if let Some(htmlPath) = args.html {
        let report = create_html_report(&duplicateSets, &fileHashes)?;
        std::fs::write(htmlPath, report)?;
    } else {
        print_text_report(&duplicateSets, &fileHashes);
    }

    Ok(())
}

fn find_all_files(root: &Path) -> AResult<Vec<PathBuf>> {
    if !root.is_dir() {
        return Err(anyhow!("given root path must be a directory"));
    }

    let mut files = vec![];
    let mut queue = VecDeque::new();
    queue.push_back(root.to_path_buf());

    while !queue.is_empty() {
        let Some(dir) = queue.pop_front() else {
            unreachable!()
        };
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                queue.push_back(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }

    Ok(files)
}

fn hash_file(file: &Path) -> AResult<String> {
    let mut hasher = sha3::Sha3_256::new();
    let mut file = std::fs::OpenOptions::new().read(true).open(file)?;
    let mut chunk = [0u8; 8192];
    loop {
        let len = file.read(&mut chunk)?;
        if len == 0 {
            break;
        }

        let slice = &chunk[..len];
        hasher.update(slice);
    }

    let mut digest = String::new();
    for byte in hasher.finalize() {
        write!(&mut digest, "{:02X}", byte)?;
    }
    Ok(digest)
}

fn print_text_report(
    duplicateSets: &HashMap<InsensitivePath, Vec<PathBuf>>,
    hashes: &HashMap<PathBuf, String>,
) {
    for (path, instances) in duplicateSets {
        println!("{:?}", path.0);
        for instance in instances {
            let hash = hashes
                .get(instance)
                .map(String::as_str)
                .unwrap_or("missing");
            println!(" => {instance:?} {hash}");
        }
    }
}

fn create_html_report(
    duplicateSets: &HashMap<InsensitivePath, Vec<PathBuf>>,
    hashes: &HashMap<PathBuf, String>,
) -> AResult<String> {
    let mut res = String::new();
    writeln!(&mut res, "<style>")?;
    writeln!(
        &mut res,
        "table {{ border-collapse: collapse; width: 100%; }}"
    )?;
    writeln!(&mut res, "td:first-child {{ width: 100%; }}")?;
    writeln!(&mut res, "table, tr, th, td {{ border: 1px solid black; }}")?;
    writeln!(&mut res, "</style>")?;
    for (path, instances) in duplicateSets {
        writeln!(&mut res, "<h3>{:?}</h3>", path.0)?;
        writeln!(&mut res, "<table>")?;
        writeln!(&mut res, "<tr><th>path</th><th>hash</th></tr>")?;
        for instance in instances {
            let hash = hashes
                .get(instance)
                .map(String::as_str)
                .unwrap_or("missing");
            writeln!(&mut res, "<tr><td>{instance:?}</td>\n<td>{hash}</td></tr>")?;
        }
        writeln!(&mut res, "</table>")?;
    }
    Ok(res)
}
