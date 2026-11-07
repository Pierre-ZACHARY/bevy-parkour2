use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};
use walkdir::WalkDir;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Spec {
    pub id: String,
    pub host: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub format: String,
    pub file: String,
}
pub fn specs() -> Result<Vec<Spec>> {
    Ok(serde_json::from_str(include_str!("../tools.lock.json"))?)
}
pub fn spec(id: &str, host: &str) -> Result<Spec> {
    specs()?
        .into_iter()
        .find(|s| s.id == id && s.host == host)
        .ok_or_else(|| anyhow::anyhow!("no pinned {id} download for {host}"))
}
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn safe_relative(path: &Path) -> Result<&Path> {
    anyhow::ensure!(!path.as_os_str().is_empty(), "empty relative path");
    anyhow::ensure!(
        path.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
        "unsafe relative path {}",
        path.display()
    );
    // A backslash is a separator on Windows, even when an archive was prepared on Unix.
    anyhow::ensure!(
        !path.to_string_lossy().contains(['\\', ':']),
        "non-portable relative path {}",
        path.display()
    );
    Ok(path)
}
pub fn inventory(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(dir).follow_links(false) {
        let entry = entry?;
        anyhow::ensure!(
            !entry.file_type().is_symlink(),
            "symlink in package/cache: {}",
            entry.path().display()
        );
        if entry.file_type().is_file() {
            let name = entry
                .path()
                .strip_prefix(dir)?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(name, hash_file(entry.path())?);
        }
    }
    Ok(files)
}
pub fn find_file(dir: &Path, name: &str) -> Result<PathBuf> {
    let found: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.file_name() == std::ffi::OsStr::new(name))
        .map(|e| e.into_path())
        .collect();
    anyhow::ensure!(
        found.len() == 1,
        "expected one {name} in {}, found {}",
        dir.display(),
        found.len()
    );
    Ok(found[0].clone())
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    archive_sha256: String,
    files: BTreeMap<String, String>,
}
fn installed(base: &Path, spec: &Spec) -> Result<PathBuf> {
    let receipt: Receipt = serde_json::from_slice(&fs::read(base.join("receipt.json"))?)?;
    anyhow::ensure!(receipt.archive_sha256 == spec.sha256, "cache pin mismatch");
    let actual = inventory(&base.join("content"))?;
    anyhow::ensure!(
        actual == receipt.files,
        "corrupt cached {} at {}",
        spec.id,
        base.display()
    );
    find_file(&base.join("content"), &spec.file)
}
pub fn cached(cache: &Path, spec: &Spec) -> Result<PathBuf> {
    if let Some(path) = provided_tool(spec)? {
        return Ok(path);
    }
    installed(
        &cache.join(&spec.id).join(&spec.version).join(&spec.host),
        spec,
    )
}
// Nix provides the exact tools.lock.json downloads with their ELF interpreter
// patched. Do not replace them with the unpatched upstream download on NixOS.
fn provided_tool(spec: &Spec) -> Result<Option<PathBuf>> {
    let Some(dir) = std::env::var_os("XTASK_TOOLS_DIR") else {
        return Ok(None);
    };
    provided_tool_in(Path::new(&dir), &crate::model::host()?, spec)
}
fn provided_tool_in(dir: &Path, host: &str, spec: &Spec) -> Result<Option<PathBuf>> {
    if !matches!(
        spec.id.as_str(),
        "nextest" | "llvm-cov" | "wasm-bindgen" | "wasm-opt"
    ) || spec.host != host
        || !specs()?.contains(spec)
    {
        return Ok(None);
    }
    let path = dir.join(&spec.file);
    anyhow::ensure!(
        path.is_file(),
        "missing provisioned {}: {}",
        spec.id,
        path.display()
    );
    Ok(Some(path))
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
pub fn ensure(cache: &Path, spec: &Spec, offline: bool) -> Result<PathBuf> {
    if let Some(path) = provided_tool(spec)? {
        return Ok(path);
    }
    let parent = cache.join(&spec.id).join(&spec.version);
    let base = parent.join(&spec.host);
    if base.exists() {
        return installed(&base, spec);
    }
    anyhow::ensure!(
        !offline,
        "{} {} is absent from {}; offline mode forbids downloading",
        spec.id,
        spec.version,
        cache.display()
    );
    fs::create_dir_all(&parent)?;
    let lock_path = parent.join(format!("{}.lock", spec.host));
    let started = std::time::Instant::now();
    let mut lock = loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => break file,
            Err(e)
                if e.kind() == std::io::ErrorKind::AlreadyExists
                    && started.elapsed() < Duration::from_secs(120) =>
            {
                if base.exists() {
                    return installed(&base, spec);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(e).context(format!(
                    "cache lock {}; another installation may be running",
                    lock_path.display()
                ));
            }
        }
    };
    let _guard = Lock(lock_path);
    writeln!(lock, "{}", std::process::id())?;
    if base.exists() {
        return installed(&base, spec);
    }
    let temporary = tempfile::tempdir_in(&parent)?;
    let archive = temporary.path().join("download");
    println!("Downloading {} {} ({})", spec.id, spec.version, spec.host);
    download(&spec.url, &archive, &spec.sha256)?;
    let content = temporary.path().join("content");
    fs::create_dir(&content)?;
    if spec.format == "file" {
        safe_relative(Path::new(&spec.file))?;
        fs::copy(&archive, content.join(&spec.file))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(content.join(&spec.file), fs::Permissions::from_mode(0o755))?;
        }
    } else {
        extract(&archive, &content, &spec.format)?;
    }
    find_file(&content, &spec.file)?;
    let receipt = Receipt {
        archive_sha256: spec.sha256.clone(),
        files: inventory(&content)?,
    };
    fs::write(
        temporary.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::remove_file(&archive)?;
    // Rename inside the same filesystem: no partially installed cache is visible.
    fs::rename(temporary.path(), &base)?;
    installed(&base, spec)
}
fn download(url: &str, path: &Path, expected: &str) -> Result<()> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(300))
        .build();
    let response = agent
        .get(url)
        .call()
        .with_context(|| format!("download {url}"))?;
    let mut source = response.into_reader();
    let mut dest = File::create(path)?;
    std::io::copy(&mut source, &mut dest)?;
    dest.sync_all()?;
    let actual = hash_file(path)?;
    anyhow::ensure!(
        actual == expected,
        "download checksum mismatch: expected {expected}, got {actual}"
    );
    Ok(())
}
fn extract(archive: &Path, dir: &Path, format: &str) -> Result<()> {
    let file = File::open(archive)?;
    if format == "zip" {
        let mut zip = zip::ZipArchive::new(file)?;
        let mut links = Vec::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let relative = Path::new(entry.name()?.as_ref()).to_path_buf();
            safe_relative(&relative)?;
            let mode = entry.unix_mode().unwrap_or(0o644);
            let destination = dir.join(relative);
            if mode & 0o170000 == 0o120000 {
                anyhow::ensure!(entry.size() <= 4096, "oversized ZIP link");
                let mut target = String::new();
                entry.read_to_string(&mut target)?;
                anyhow::ensure!(
                    !Path::new(&target).is_absolute()
                        && !target.contains(['\\', ':'])
                        && !target.chars().any(char::is_control),
                    "unsafe ZIP link"
                );
                links.push((destination, target));
                continue;
            }
            if entry.is_dir() {
                fs::create_dir_all(&destination)?;
            } else {
                fs::create_dir_all(destination.parent().context("ZIP parent")?)?;
                std::io::copy(&mut entry, &mut File::create(&destination)?)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(destination, fs::Permissions::from_mode(mode & 0o777))?;
                }
            }
        }
        // JRE Linux archives contain internal license links. Materialize them as
        // ordinary copies after extraction, never create filesystem symlinks.
        let root = dir.canonicalize()?;
        while !links.is_empty() {
            let count = links.len();
            let mut pending = Vec::new();
            for (destination, target) in links {
                // On Unix, canonicalize("missing/../file") fails even when file
                // exists. ZIPs need not include their link's parent directory.
                fs::create_dir_all(destination.parent().context("ZIP link parent")?)?;
                let target = destination
                    .parent()
                    .context("ZIP link parent")?
                    .join(target);
                match target.canonicalize() {
                    Ok(target) => {
                        anyhow::ensure!(
                            target.starts_with(&root) && target.is_file(),
                            "ZIP link escapes cache or targets a directory"
                        );
                        fs::copy(target, destination)?;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        pending.push((destination, target.to_string_lossy().into_owned()))
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            anyhow::ensure!(pending.len() < count, "broken/cyclic ZIP link");
            links = pending;
        }
        return Ok(());
    }
    let reader: Box<dyn Read> = match format {
        "tar.gz" => Box::new(flate2::read::GzDecoder::new(file)),
        "tar" => Box::new(file),
        _ => bail!("unsupported archive format {format}"),
    };
    for entry in tar::Archive::new(reader).entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        safe_relative(&path)?;
        // Only regular files/directories are needed. Do not materialize symlinks,
        // hardlinks or device entries from third-party archives.
        if entry.header().entry_type().is_file() || entry.header().entry_type().is_dir() {
            anyhow::ensure!(entry.unpack_in(dir)?, "archive entry escaped destination");
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zip_links_are_copies_and_cannot_escape() -> Result<()> {
        use zip::write::SimpleFileOptions;
        let temp = tempfile::tempdir()?;
        for (index, target) in ["../source", "../../../outside", "link"].iter().enumerate() {
            let archive = temp.path().join(format!("{index}.zip"));
            let mut zip = zip::ZipWriter::new(File::create(&archive)?);
            zip.start_file("root/source", SimpleFileOptions::default())?;
            zip.write_all(b"license")?;
            zip.add_symlink("root/sub/link", *target, SimpleFileOptions::default())?;
            zip.finish()?;
            let dest = temp.path().join(index.to_string());
            fs::create_dir_all(&dest)?;
            let result = extract(&archive, &dest, "zip");
            if index == 0 {
                result?;
                assert_eq!(fs::read(dest.join("root/sub/link"))?, b"license");
                assert!(!fs::symlink_metadata(dest.join("root/sub/link"))?.is_symlink());
            } else {
                assert!(result.is_err());
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires the three downloaded scanner ZIPs in artifacts/telemetry-research"]
    fn scanner_archives_extract_on_all_hosts() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        for host in [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin",
        ] {
            let archive = root.join(format!("artifacts/telemetry-research/scanner-{host}.zip"));
            assert_eq!(hash_file(&archive)?, spec("sonar-scanner", host)?.sha256);
            let temp = tempfile::tempdir()?;
            extract(&archive, temp.path(), "zip")?;
            let jar = find_file(temp.path(), "sonar-scanner-cli-8.0.1.6346.jar")?;
            assert!(
                jar.parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("jre/bin/java")
                    .is_file()
            );
            inventory(temp.path())?;
        }
        Ok(())
    }
    #[test]
    fn all_pins_have_checksums() {
        for s in specs().unwrap() {
            assert_eq!(s.sha256.len(), 64);
            assert!(s.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(s.url.starts_with("https://"));
            assert!(!s.url.contains("/latest/"));
        }
    }
    #[test]
    fn standalone_cache_bootstrap_matches_tool_pins() {
        let specs = specs().unwrap();
        let mut count = 0;
        for line in include_str!("../../compiler-cache-pins.txt").lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (host, checksum) = line.split_once(' ').unwrap();
            let spec = specs
                .iter()
                .find(|s| s.id == "sccache" && s.host == host)
                .unwrap();
            assert_eq!(spec.sha256, checksum);
            assert_eq!(spec.version, "0.19.0");
            count += 1;
        }
        assert_eq!(count, specs.iter().filter(|s| s.id == "sccache").count());
    }
    #[test]
    fn traversal_is_rejected() {
        for path in [
            "../escape",
            "/absolute",
            "C:/escape",
            "a\\..\\escape",
            "a/../../escape",
        ] {
            assert!(safe_relative(Path::new(path)).is_err());
        }
    }
    #[test]
    fn corruption_and_offline_miss_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = spec("nextest", "x86_64-pc-windows-msvc").unwrap();
        assert!(ensure(tmp.path(), &spec, true).is_err());
        let base = tmp
            .path()
            .join(&spec.id)
            .join(&spec.version)
            .join(&spec.host);
        fs::create_dir_all(base.join("content")).unwrap();
        fs::write(base.join("content").join(&spec.file), "original").unwrap();
        let r = Receipt {
            archive_sha256: spec.sha256.clone(),
            files: inventory(&base.join("content")).unwrap(),
        };
        fs::write(base.join("receipt.json"), serde_json::to_vec(&r).unwrap()).unwrap();
        assert!(cached(tmp.path(), &spec).is_ok());
        fs::write(base.join("content").join(&spec.file), "corrupt").unwrap();
        assert!(cached(tmp.path(), &spec).is_err());
        assert!(ensure(tmp.path(), &spec, false).is_err());
    }
    #[test]
    fn plain_file_download_is_verified_and_reused_offline() {
        use std::net::TcpListener;
        let payload = b"globalThis.Sentry = {};\n";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let _ = socket.read(&mut request);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .unwrap();
            socket.write_all(payload).unwrap();
        });
        let tmp = tempfile::tempdir().unwrap();
        let mut spec = spec("sentry-browser", "universal").unwrap();
        spec.url = format!("http://{addr}/sdk.js");
        spec.sha256 = format!("{:x}", Sha256::digest(payload));
        let path = ensure(tmp.path(), &spec, false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), payload);
        server.join().unwrap();
        assert_eq!(ensure(tmp.path(), &spec, true).unwrap(), path);
        fs::write(&path, "tampered").unwrap();
        assert!(ensure(tmp.path(), &spec, true).is_err());
    }
    #[test]
    fn interrupted_download_never_installs() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let _ = socket.read(&mut request);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 99999\r\nConnection: close\r\n\r\npartial",
                )
                .unwrap();
        });
        let tmp = tempfile::tempdir().unwrap();
        let mut spec = spec("nextest", "x86_64-pc-windows-msvc").unwrap();
        spec.url = format!("http://{addr}/fixture");
        assert!(ensure(tmp.path(), &spec, false).is_err());
        assert!(
            !tmp.path()
                .join(&spec.id)
                .join(&spec.version)
                .join(&spec.host)
                .exists()
        );
        server.join().unwrap();
    }
}
#[test]
fn provisioned_tools_only_replace_the_exact_upstream_pin() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let mut pin = spec("nextest", "x86_64-unknown-linux-gnu")?;
    let host = "x86_64-unknown-linux-gnu";
    assert!(provided_tool_in(temp.path(), host, &pin).is_err());
    let binary = temp.path().join(&pin.file);
    fs::write(&binary, "provided tool")?;
    assert_eq!(provided_tool_in(temp.path(), host, &pin)?, Some(binary));
    assert_eq!(
        provided_tool_in(temp.path(), "aarch64-unknown-linux-gnu", &pin)?,
        None
    );
    pin.url = "http://127.0.0.1:1/interrupted-fixture".into();
    assert_eq!(provided_tool_in(temp.path(), host, &pin)?, None);
    Ok(())
}
