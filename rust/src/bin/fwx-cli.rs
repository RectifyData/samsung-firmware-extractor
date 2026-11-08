//! Command-line front end for the firmware extractor (Windows, Linux, macOS).
//!
//!   fwx-cli list    <firmware>...
//!   fwx-cli extract [--out DIR] [--match TEXT] <firmware>...

use anyhow::{bail, Context, Result};
use fwx::{extract_images, list_images, Events};
use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

const USAGE: &str = "\
fwx-cli: extract raw .img files from Samsung firmware

USAGE:
  fwx-cli list    <firmware>...
  fwx-cli extract [--out DIR] [--match TEXT] <firmware>...

  --out DIR     output root (default: next to each firmware file)
  --match TEXT  only images whose path contains TEXT (case-insensitive)

Each firmware gets its own <name>_img folder under the output root.
Android sparse images are converted to raw automatically.";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<()> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        println!("{USAGE}");
        return Ok(());
    };
    if matches!(command.as_str(), "-h" | "--help" | "help") {
        println!("{USAGE}");
        return Ok(());
    }

    let mut out: Option<PathBuf> = None;
    let mut pattern: Option<String> = None;
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(args.next().context("--out needs a folder")?)),
            "--match" => pattern = Some(args.next().context("--match needs text")?.to_lowercase()),
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if files.is_empty() {
        bail!("no firmware files given\n\n{USAGE}");
    }

    match command.as_str() {
        "list" => {
            for file in &files {
                println!("{}", file.display());
                for image in list_images(open(file)?, &name_of(file), &AtomicBool::new(false))? {
                    let size = image.size.map(|s| format!("{:.1} MiB", s as f64 / 1048576.0));
                    println!("  {}  {}", size.unwrap_or_else(|| "?".into()), image.path);
                }
            }
            Ok(())
        }
        "extract" => {
            for file in &files {
                extract_one(file, out.as_deref(), pattern.as_deref())?;
            }
            Ok(())
        }
        other => bail!("unknown command `{other}`\n\n{USAGE}"),
    }
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

    let base = match out_root {
        Some(root) => root.to_path_buf(),
        None => file.parent().unwrap_or(Path::new(".")).to_path_buf(),
    };
    let out_dir = base.join(format!("{}_img", name.split('.').next().unwrap_or(&name)));
    fs::create_dir_all(&out_dir).with_context(|| format!("cannot create {}", out_dir.display()))?;
    println!("== {name}: {} image(s) -> {}", selected.len(), out_dir.display());

    let work = out_dir.join(".work");
    let mut events = Printer { out_dir: out_dir.clone() };
    extract_images(open(file)?, &name, &selected, &work, &mut events, &cancel)?;
    let _ = fs::remove_dir_all(&work);
    Ok(())
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
