#![allow(unused)]

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use std::fmt::format;
use std::fs::{read_dir, File};
use std::io::{self, BufRead, Write};
use std::path::Path;
use tokio::task;

/// Search for interpolation errors in .po files and display the lines containing them.
#[derive(Parser)]
struct Args {
    /// The path to the .po files folder
    path: std::path::PathBuf,
    /// The regex pattern to match translation interpolations
    #[arg(short, long, default_value_t = String::from(r"\{\{.*\}\}|\{.*\}"))]
    pattern: String,
}

struct InterpolationParams<'a> {
    pb: &'a ProgressBar,
    path: &'a Path,
    pattern: &'a String,
    last_msgid: &'a str,
    line: &'a str,
    line_index: u32,
}

fn find_missing_interpolations(params: InterpolationParams) -> Option<String> {
    let regex = Regex::new(&params.pattern).unwrap();
    let msgid_interpolations: Vec<_> = regex.find_iter(params.last_msgid).collect();
    let msgstr_interpolations: Vec<_> = regex.find_iter(params.line).collect();
    if msgid_interpolations.len() != msgstr_interpolations.len() {
        for cap in msgid_interpolations {
            let regex_match = &cap.as_str();
            if !params.line.contains(regex_match)
                && params.line != "msgstr \"\""
                && params.line != "\"\""
            {
                return Some(format!(
                    "{}\x1b[31m[ERROR] Missing interpolation in {}:{}\n\t{}\n\t{}\x1b[0m",
                    params.pb.message(),
                    params.path.display(),
                    params.line_index,
                    params.last_msgid,
                    params.line
                ));
            }
        }
    }
    None
}

fn process_file(pb: &ProgressBar, path: &Path, pattern: &String) -> io::Result<Vec<String>> {
    let file = File::open(&path)?;
    let reader = io::BufReader::new(file);
    let mut last_msgid = String::new();
    let mut lines = reader.lines();
    let mut line_index = 1;
    let mut errors = Vec::new();
    for line in lines {
        let mut line = line?;
        if line.starts_with("msgid") {
            last_msgid = line.clone();
        } else if line.starts_with("msgstr") || line.starts_with("\"") {
            let params = InterpolationParams {
                pb: &pb,
                path: &path,
                pattern: &pattern,
                last_msgid: &last_msgid,
                line: &line,
                line_index,
            };
            if let Some(error) = find_missing_interpolations(params) {
                errors.push(error);
            }
        }
        line_index += 1
    }
    pb.inc(1);
    Ok(errors)
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    let dir = Path::new(args.path.to_str().unwrap());
    let entries = read_dir(dir)?.count();
    let pb = ProgressBar::new(entries as u64);
    let mut has_po_files = false;

    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner} [{bar:40}] {pos}/{len} files ({eta})")
            .unwrap()
            .progress_chars("=>-"),
    );

    let mut tasks = vec![];
    for entry in read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("po") {
            has_po_files = true;
            let pb = pb.clone();
            let pattern = args.pattern.clone();
            tasks.push(task::spawn(
                async move { process_file(&pb, &path, &pattern) },
            ));
        }
    }

    if !has_po_files {
        pb.println(format!(
            "\x1b[0;31m[ERROR] No .po files found in {}\x1b[0m",
            dir.display()
        ));
        pb.finish_and_clear();
        std::process::exit(1);
    } else {
        pb.println(format!(
            "\x1b[0;36m[INFO]  Processing .po files in {}\x1b[0m",
            dir.display()
        ));
    }

    let mut all_errors = vec![];
    for task in tasks {
        match task.await {
            Ok(Ok(errors)) => {
                for error in &errors {
                    pb.println(error);
                }
                all_errors.extend(errors)
            }
            Ok(Err(e)) => return Err(e),
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }

    if !all_errors.is_empty() {
        pb.finish_and_clear();
        std::process::exit(1);
    } else {
        pb.println("\x1b[0;36m[INFO]  Done\x1b[0m");
    }

    Ok(())
}
