//! fwx-cli: extract raw .img files from Samsung firmware. Runs on Windows, macOS and Linux.
//!
//! Exit codes: 0 success, 1 runtime error (missing file, bad firmware, I/O), 2 usage error.

use anyhow::{anyhow, Context, Result};
use fwx::{extract_images, list_images, Events};
use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

const VERSION: &str = env!("CARGO_PKG_VERSION");

enum Failure {
    /// Bad command line: printed with usage, exit code 2.
    Usage(String),
    /// Something went wrong while working: exit code 1.
    Runtime(anyhow::Error),
}

fn usage() -> String {
    format!(
        "\
fwx-cli {VERSION}: extract raw .img files from Samsung firmware

USAGE:
  fwx-cli list    <firmware>...
  fwx-cli extract [-o DIR] [-m TEXT] <firmware>...
  fwx-cli --version | --help

COMMANDS:
  list     show the lz4 images inside each firmware file
  extract  decode the images to raw .img files

OPTIONS:
  -o, --out DIR     output root (default: next to each firmware file)
  -m, --match TEXT  only images whose path contains TEXT (case-insensitive)
  -h, --help        show this help
  -V, --version     show the version

Each firmware gets its own <name>_img folder under the output root.
Android sparse images are converted to raw automatically.
Firmware can be .zip, .tar, .tar.md5, or a single .lz4 file."
    )
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(message)) => {
            eprintln!("error: {message}\n\n{}", usage());
            ExitCode::from(2)
        }
        Err(Failure::Runtime(e)) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<String>) -> Result<(), Failure> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Err(Failure::Usage("no command given".into()));
    };
    match command.as_str() {
        "-h" | "--help" | "help" => {
            println!("{}", usage());
            return Ok(());
        }
        "-V" | "--version" | "version" => {
            println!("fwx-cli {VERSION}");
            return Ok(());
        }
        "list" | "extract" => {}
        other => return Err(Failure::Usage(format!("unknown command `{other}`"))),
    }

    let mut out: Option<PathBuf> = None;
    let mut pattern: Option<String> = None;
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--out" => out = Some(PathBuf::from(value(&mut args, &arg)?)),
            "-m" | "--match" => pattern = Some(value(&mut args, &arg)?.to_lowercase()),
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(Failure::Usage(format!("unknown option `{flag}`")));
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if files.is_empty() {
        return Err(Failure::Usage("no firmware files given".into()));
    }

    if command == "list" {
        for file in &files {
            list_one(file).map_err(Failure::Runtime)?;
        }
        return Ok(());
    }

    // extract: keep going after a bad file, then report how many failed.
    let mut failed = 0;
    for file in &files {
        if let Err(e) = extract_one(file, out.as_deref(), pattern.as_deref()) {
            eprintln!("error: {}: {e:#}", file.display());
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(Failure::Runtime(anyhow!("{failed} of {} firmware file(s) failed", files.len())));
    }
    Ok(())
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, Failure> {
    args.next().ok_or_else(|| Failure::Usage(format!("{flag} needs a value")))
}

fn list_one(file: &Path) -> Result<()> {
    let name = name_of(file);
    println!("{}", file.display());
    let images = list_images(open(file)?, &name, &AtomicBool::new(false))?;
    if images.is_empty() {
        println!("  (no lz4 images found)");
    }
    for image in images {
        let size = match image.size {
            Some(s) => format!("{:>9.1} MiB", s as f64 / 1048576.0),
            None => format!("{:>13}", "?"),
        };
        println!("  {size}  {}", image.path);
    }
    Ok(())
}

fn extract_one(file: &Path, out_root: Option<&Path>, pattern: Option<&str>) -> Result<()> {
    let name = name_of(file);
    let cancel = AtomicBool::new(false);

    let selected: HashSet<String> = list_images(open(file)?, &name, &cancel)?
        .into_iter()
        .map(|i| i.path)
        .filter(|p| pattern.map_or(true, |t| p.to_lowercase().contains(t)))
        .collect();
    if selected.is_empty() {
        println!("{name}: no matching images");
        return Ok(());
    }

    let base = out_root.unwrap_or_else(|| file.parent().unwrap_or(Path::new(".")));
    let out_dir = base.join(format!("{}_img", name.split('.').next().unwrap_or(&name)));
    fs::create_dir_all(&out_dir).with_context(|| format!("cannot create {}", out_dir.display()))?;
    println!("== {name}: {} image(s) -> {}", selected.len(), out_dir.display());

    let work = out_dir.join(".work");
    let mut printer = Printer { out_dir: out_dir.clone() };
    let result = extract_images(open(file)?, &name, &selected, &work, &mut printer, &cancel);
    let _ = fs::remove_dir_all(&work);
    result
}

struct Printer {
    out_dir: PathBuf,
}

impl Events for Printer {
    fn log(&mut self, message: &str) {
        println!("{message}");
    }

    fn image(&mut self, name: &str, path: &Path) -> Result<()> {
        let dest = self.out_dir.join(name);
        fs::copy(path, &dest).with_context(|| format!("cannot write {}", dest.display()))?;
        println!("  wrote {}", dest.display());
        Ok(())
    }
}

fn open(path: &Path) -> Result<File> {
    File::open(path).with_context(|| format!("cannot open {}", path.display()))
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
