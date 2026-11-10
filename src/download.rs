//! Download a file and unzip it.
use crate::Res;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Download `url` into `dest`, reporting progress (0.0 to 1.0) when the size is known.
pub fn fetch_to_file(url: &str, dest: &Path, progress: &dyn Fn(f32)) -> Res<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let resp = ureq::get(url).call()?;
    let total: Option<u64> = resp.header("Content-Length").and_then(|v| v.parse().ok());
    let mut reader = resp.into_reader();
    let mut out = fs::File::create(dest)?;

    let mut buf = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        if let Some(t) = total {
            progress(done as f32 / t as f32);
        }
    }
    Ok(())
}

/// Unzip `zip_path` into `target`, dropping the first `strip` folder levels of every path.
/// (Google's zip has a top "cmdline-tools/" folder we don't want; same for the Java zip.)
pub fn extract_zip(zip_path: &Path, target: &Path, strip: usize) -> Res<()> {
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    fs::create_dir_all(target)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        // enclosed_name() refuses paths like "../../evil" (zip-slip protection).
        let Some(rel) = entry.enclosed_name() else { continue };
        let rel: PathBuf = rel.components().skip(strip).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out_path = target.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = fs::File::create(&out_path)?;
            io::copy(&mut entry, &mut f)?;
        }
    }
    Ok(())
}
