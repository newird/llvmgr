mod llvm;

use std::{
    collections::{HashMap, VecDeque},
    fmt::format,
    fs::{self, read_to_string, File, OpenOptions},
    io::{self, BufRead, BufReader, Cursor, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use color_eyre::{
    eyre::{Context as EyreContext, ContextCompat},
    Help, Report,
};
use fs_extra::dir::CopyOptions;
use reqwest::IntoUrl;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::tasks::TaskRef;

pub(crate) mod install;

#[derive(Error, Debug)]
pub(crate) enum FileSystemError {
    #[error("cannot retrieve user directory")]
    UserDirError,
    #[error("{0}")]
    IO(std::io::Error),
    #[error("{0}")]
    CannotMove(fs_extra::error::Error),
    #[error("{0}")]
    CannotRemove(fs_extra::error::Error),
}

fn cache_root() -> Result<PathBuf, FileSystemError> {
    let dirs = directories::UserDirs::new().ok_or(FileSystemError::UserDirError)?;
    let root = dirs.home_dir().join(".cache/llvmgr");
    std::fs::create_dir_all(&root).map_err(FileSystemError::IO)?;
    Ok(root)
}

fn mkdir_inside_cache_folder(path: impl AsRef<Path>) -> Result<PathBuf, FileSystemError> {
    let dirs = directories::UserDirs::new().ok_or(FileSystemError::UserDirError)?;
    let p = dirs.home_dir().join(".cache/llvmgr").join(path);
    std::fs::create_dir_all(&p).map_err(FileSystemError::IO)?;
    Ok(p)
}

pub(crate) fn cache_path(path: impl AsRef<Path>) -> Result<PathBuf, FileSystemError> {
    let dirs = directories::UserDirs::new().ok_or(FileSystemError::UserDirError)?;
    let p = dirs.home_dir().join(".cache/llvmgr").join(path);
    Ok(p)
}

fn set_current_dir_inside_cache_folder(path: impl AsRef<Path>) -> Result<(), FileSystemError> {
    let p = mkdir_inside_cache_folder(path)?;
    std::env::set_current_dir(p).map_err(FileSystemError::IO)
}

pub struct DownloadResult {
    path: PathBuf,
}

#[derive(Error, Debug)]
pub(crate) enum DownloadError {
    #[error("cache unavailable: {0}")]
    CacheUnavailable(FileSystemError),
    #[error("url {0}")]
    InvalidUrl(String),
    #[error("http error")]
    Reqwest(reqwest::Error),
    #[error("http error")]
    Http(reqwest::StatusCode),
    #[error("Content-Length header {0}")]
    ContentLength(String),
    #[error("io error")]
    IO(tokio::io::Error),
}

async fn download(
    t: &TaskRef,
    url: impl reqwest::IntoUrl,
) -> Result<DownloadResult, DownloadError> {
    t.set_subtask("downloading");

    let url = url
        .into_url()
        .map_err(|err| DownloadError::InvalidUrl(err.to_string()))?;

    let file_name = url
        .path_segments()
        .ok_or_else(|| DownloadError::InvalidUrl("url does not have segments".to_string()))?
        .last()
        .ok_or_else(|| DownloadError::InvalidUrl("url does not have segments".to_string()))?;

    let cache_root = cache_root().map_err(DownloadError::CacheUnavailable)?;
    let cache_file_path = cache_root.join(file_name);
    if cache_file_path.exists() {
        return Ok(DownloadResult {
            path: cache_file_path,
        });
    }

    let mut cache_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&cache_file_path)
        .await
        .map_err(DownloadError::IO)?;

    let req = reqwest::get(url).await.map_err(DownloadError::Reqwest)?;
    let status = req.status();
    if !status.is_success() {
        return Err(DownloadError::Http(status));
    }

    let content_length = req
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|x| x.to_str().ok()?.parse::<f64>().ok());

    use futures_util::StreamExt;
    let mut completed = 0.0;
    let mut stream = req.bytes_stream();
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(DownloadError::Reqwest)?;
        cache_file
            .write_all(&bytes)
            .await
            .map_err(DownloadError::IO)?;

        if let Some(content_length) = content_length.as_ref() {
            completed += bytes.len() as f64 / content_length;
            t.set_percentage(completed)
        } else {
            completed += bytes.len() as f64;
            t.set_subtask(&format!("{} bytes", completed));
        }
    }

    Ok(DownloadResult {
        path: cache_file_path,
    })
}

#[derive(Error, Debug)]
pub(crate) enum UnxzError {
    #[error("{0}")]
    IO(std::io::Error),
}

pub(crate) async fn ungz(t: &TaskRef, path: impl AsRef<Path>) -> Result<Vec<u8>, std::io::Error> {
    t.set_subtask("ungz-ing");

    let f = std::fs::File::options().read(true).open(path)?;

    let metadata = f.metadata()?;
    let total = metadata.len() as f64;

    let mut f = libflate::gzip::Decoder::new(f)?;

    let mut out = vec![];

    let mut buffer = [0u8; 16 * 1024];
    loop {
        let s = f.read(&mut buffer)?;
        if s == 0 {
            break;
        }
        out.extend(&buffer[0..s]);

        let inner_pos = f.as_inner_ref().seek(std::io::SeekFrom::Current(0))?;
        t.set_percentage(inner_pos as f64 / total)
    }

    Ok(out)
}

pub(crate) async fn unxz(t: &TaskRef, path: impl AsRef<Path>) -> Result<Vec<u8>, UnxzError> {
    t.set_subtask("unxz-ing");

    let f = std::fs::File::options()
        .read(true)
        .open(path)
        .map_err(UnxzError::IO)?;
    let metadata = f.metadata().map_err(UnxzError::IO)?;

    let total = metadata.len() as f64;

    let mut f = xz2::read::XzDecoder::new(f);

    let mut out = vec![];

    let mut buffer = [0u8; 16 * 1024];
    loop {
        let s = f.read(&mut buffer).map_err(UnxzError::IO)?;
        if s == 0 {
            break;
        }
        out.extend(&buffer[0..s]);

        t.set_percentage(f.total_in() as f64 / total)
    }

    Ok(out)
}

#[derive(Error, Debug)]
pub(crate) enum UntarError {
    #[error("invalid destination")]
    InvalidDest,
    #[error("{0} {1}")]
    IO(&'static str, std::io::Error),
}

pub(crate) fn untar_from_vec(
    t: &TaskRef,
    v: Vec<u8>,
    dest: impl AsRef<Path>,
) -> Result<(), UntarError> {
    t.set_subtask("untar-ing");

    let dest = dest.as_ref();
    std::fs::create_dir_all(dest).map_err(|err| UntarError::IO("create_dir_all dest", err))?;

    // Get how many entries are
    let v = Cursor::new(v);
    let mut f = tar::Archive::new(v);
    let entries = f.entries().map_err(|err| UntarError::IO("entries", err))?;
    let len = entries.count() as f64;

    // actualy read the entries
    let v = f.into_inner().into_inner();
    let v = Cursor::new(v);
    let mut f = tar::Archive::new(v);

    for (i, entry) in f
        .entries()
        .map_err(|err| UntarError::IO("entries", err))?
        .enumerate()
    {
        let mut entry = entry.map_err(|err| UntarError::IO("entry", err))?;

        if !entry.header().entry_type().is_file() {
            continue;
        }

        let path = entry.path().map_err(|err| UntarError::IO("path", err))?;
        let rel_path = path.components().skip(1);

        let mut dest = dest.to_path_buf();
        for p in rel_path {
            dest.push(p);
        }

        let parent = dest.parent().ok_or(UntarError::InvalidDest)?;
        std::fs::create_dir_all(parent)
            .map_err(|err| UntarError::IO("create_dir_all parent", err))?;

        let mut bytes = vec![];
        entry
            .read_to_end(&mut bytes)
            .map_err(|err| UntarError::IO("read_to_end", err))?;
        std::fs::write(dest, bytes).map_err(|err| UntarError::IO("write", err))?;

        t.set_percentage(i as f64 / len);
    }

    Ok(())
}

#[derive(Error, Debug)]
pub(crate) enum DownloadDecompressError {
    #[error("download: {0}")]
    Download(DownloadError),
    #[error("unxz: {0}")]
    Unxz(UnxzError),
    #[error("ungz: {0}")]
    Ungz(std::io::Error),
    #[error("untar: {0}")]
    Untar(UntarError),
}

pub(crate) async fn download_unxz_untar(
    t: &TaskRef,
    url: impl IntoUrl,
    dest: impl AsRef<Path>,
) -> Result<(), DownloadDecompressError> {
    let llvm_tar_xz = download(t, url)
        .await
        .map_err(DownloadDecompressError::Download)?;
    let llvm_tar = unxz(t, &llvm_tar_xz.path)
        .await
        .map_err(DownloadDecompressError::Unxz)?;
    untar_from_vec(t, llvm_tar, dest).map_err(DownloadDecompressError::Untar)?;

    Ok(())
}

pub(crate) async fn download_ungz_untar(
    t: &TaskRef,
    url: impl IntoUrl,
    dest: impl AsRef<Path>,
) -> Result<PathBuf, DownloadDecompressError> {
    let llvm_tar_gz = download(t, url)
        .await
        .map_err(DownloadDecompressError::Download)?;
    let llvm_tar_gz_file_path = llvm_tar_gz.path.to_path_buf();
    let llvm_tar = ungz(t, &llvm_tar_gz.path)
        .await
        .map_err(DownloadDecompressError::Ungz)?;
    untar_from_vec(t, llvm_tar, dest).map_err(DownloadDecompressError::Untar)?;

    Ok(llvm_tar_gz_file_path)
}

// add some code to line of the file
pub fn add_line_to_file(path: &Path, content: String, line: u32) -> Result<(), Report> {
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    let reader = BufReader::new(&file);

    let mut lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

    let index = if line > 0 { (line - 1) as usize } else { 0 };
    if index <= lines.len() {
        lines.insert(index, content);
    } else {
        lines.resize(index, String::new());
        lines.push(content);
    }

    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;

    for line_content in lines {
        writeln!(file, "{}", line_content)?;
    }
    Ok(())
}
// parses strings like: "[179/3416]"
fn is_progress(line: &str) -> nom::IResult<&str, (usize, usize)> {
    let (line, _) = nom::bytes::complete::tag("[")(line)?;
    let (line, current) = nom::combinator::map_opt(nom::character::complete::digit1, |s: &str| {
        s.parse::<usize>().ok()
    })(line)?;
    let (line, _) = nom::bytes::complete::tag("/")(line)?;
    let (line, total) = nom::combinator::map_opt(nom::character::complete::digit1, |s: &str| {
        s.parse::<usize>().ok()
    })(line)?;

    Ok((line, (current, total)))
}

#[derive(Error, Debug)]
pub(crate) enum SpawnError {
    #[error("command not found")]
    CommandNotFound,
    #[error("{0}")]
    IO(std::io::Error),
}

pub(crate) fn spawn_cmake<I, S>(t: &TaskRef, args: I) -> Result<(), Report>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let exe = search_cmake().ok_or(SpawnError::CommandNotFound)?;

    let mut process = std::process::Command::new(&exe)
        .args(args)
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(SpawnError::IO)?;

    let log_path = cache_root().wrap_err("no cache")?.join("compile.log");
    let mut log_file = fs::File::create(log_path)?;

    let mut last_percentage = 0.0;

    if let Some(stdout) = process.stdout.take() {
        let lines = BufReader::new(stdout);

        for line in lines.lines().map_while(Result::ok) {
            writeln!(&mut log_file, "{}", line)?;
            if let Ok((_, (current, total))) = is_progress(&line) {
                last_percentage = current as f64 / total as f64;
            }

            t.set_subtask_with_percentage(&line, last_percentage);
        }
    }

    process.wait().map_err(SpawnError::IO)?;

    t.set_subtask_with_percentage("", 1.0);

    Ok(())
}
#[derive(Default, Serialize, Deserialize)]
pub struct Shell {
    pub env_vars: HashMap<String, String>,
}

#[derive(Error, Debug)]
pub(crate) enum ReadShellError {
    #[error("{0}")]
    IO(std::io::Error),
    #[error("{0}")]
    FileSystem(FileSystemError),
    #[error("{0}")]
    Serialization(serde_json::Error),
}

pub(crate) fn read_shell() -> Result<Shell, ReadShellError> {
    let shell_path = cache_root()
        .map_err(ReadShellError::FileSystem)?
        .join("shell");

    let shell = std::fs::read_to_string(shell_path)
        .or_else(|_| serde_json::to_string(&Shell::default()))
        .map_err(ReadShellError::Serialization)?;

    serde_json::from_str(&shell).map_err(ReadShellError::Serialization)
}

pub(crate) fn write_shell(shell: &Shell) -> Result<(), ReadShellError> {
    let shell_path = cache_root()
        .map_err(ReadShellError::FileSystem)?
        .join("shell");
    let shell = serde_json::to_string_pretty(&shell).expect("this should not fail");
    std::fs::write(shell_path, shell).map_err(ReadShellError::IO)
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct Config {
    cache: HashMap<String, String>,
    compile_config: HashMap<String, String>,
    cmake: HashMap<String, String>,
}

impl Config {
    pub fn cmake_args(&self) -> Vec<String> {
        let mut cmake_args = self
            .cmake
            .iter()
            .map(|(k, v)| format!("-{} {}", k, v))
            .collect::<Vec<String>>();
        let compile_config_args = self
            .compile_config
            .iter()
            .map(|(k, v)| format!("-D{}={}", k, v))
            .collect::<Vec<String>>();

        cmake_args.extend(compile_config_args);
        cmake_args
    }
}
pub(crate) fn init_config() -> Result<(), Report> {
    let config_path = cache_root()?.join("config.toml");

    let config_exist = fs::exists(&config_path).wrap_err("file system error")?;
    if config_exist {
        return Ok(());
    }
    let mut config = Config::default();
    // Example default values

    let cmake = search_cmake()
        .wrap_err("'cmake' cannot be found")
        .with_suggestion(suggest_install_cmake)?;
    let generator = get_cmake_default_generator(cmake)?;

    config
        .cache
        .insert("delete_src".to_string(), "false".to_string());
    config
        .cache
        .insert("delete_xz".to_string(), "false".to_string());
    if generator.contains("Visual Studio") {
        config
            .cmake
            .insert("G".to_string(), "Visual Studio".to_string());
    } else {
        config.cmake.insert("G".to_string(), "Ninja".to_string());
    }
    config.cmake.insert("B".to_string(), "build".to_string());
    config.cmake.insert("S".to_string(), "llvm".to_string());
    config
        .compile_config
        .insert("CMAKE_BUILD_TYPE".to_string(), "Release".to_string());
    config.compile_config.insert(
        "LLVM_ENABLE_PROJECTS".to_string(),
        "\"clang;lld\"".to_string(),
    );
    config
        .compile_config
        .insert("LLVM_TARGETS_TO_BUILD".to_string(), "X86".to_string());
    config
        .compile_config
        .insert("LLVM_INSTALL_PREFIX".to_string(), "install".to_string());

    let toml_string =
        toml::to_string(&config).wrap_err_with(|| format!("failed to read from {:?}", config))?;
    fs::write(&config_path, toml_string)
        .wrap_err_with(|| format!("can't write to {:?}", config_path.display()))
}

pub(crate) fn read_config() -> Result<Config, Report> {
    init_config()?;
    let root_path = cache_root().wrap_err("Can't find the cache directory")?;
    let config_path = root_path.join("config.toml");

    let config_str =
        read_to_string(&config_path).wrap_err("Failed to read the configuration file")?;

    let config: Config =
        toml::from_str(&config_str).wrap_err("Failed to parse the configuration file")?;

    Ok(config)
}

pub(crate) fn move_dir(
    src: impl AsRef<Path>,
    dest: impl AsRef<Path>,
) -> Result<(), FileSystemError> {
    let options = CopyOptions::default().overwrite(true);
    let _ = fs_extra::move_items(&[src], dest, &options).map_err(FileSystemError::CannotMove);

    Ok(())
}

pub(crate) fn remove_dir(dir: impl AsRef<Path>) -> Result<(), FileSystemError> {
    let _ = fs_extra::remove_items(&[dir]).map_err(FileSystemError::CannotRemove);

    Ok(())
}

pub(crate) fn search_cmake() -> Option<PathBuf> {
    let cmake = which::which("cmake");
    let cmake = if cmake.is_ok() {
        cmake
    } else {
        #[cfg(target_os = "linux")]
        {
            cmake
        }
        #[cfg(target_os = "windows")]
        {
            let path: PathBuf = "C:\\Program Files\\CMake\\bin\\cmake.exe".into();
            if path.exists() {
                Ok(path)
            } else {
                cmake
            }
        }
    };

    cmake.ok()
}

pub(crate) fn suggest_install_cmake() -> String {
    #[cfg(target_os = "linux")]
    {
        "Install cmake`".into()
    }
    #[cfg(target_os = "windows")]
    {
        "If chocolatey is installed, one can install cmake with `choco install cmake`".into()
    }
}

pub(crate) fn get_cmake_default_generator(cmake: PathBuf) -> Result<String, Report> {
    let output = std::process::Command::new(&cmake)
        .args(["--help"])
        .stdout(std::process::Stdio::piped())
        .output()
        .map_err(SpawnError::IO)?;

    for line in output.stdout.lines().flatten() {
        if let Some(generator) = line.strip_prefix("* ").and_then(|s| s.split('=').next()) {
            return Ok(generator.trim().into());
        }
    }

    #[cfg(target_os = "linux")]
    {
        Err(color_eyre::eyre::eyre!("No defaut generator installed"))
            .wrap_err("cmake has not found any generator")
            .with_suggestion(|| "Install `ninja`")
    }
    #[cfg(target_os = "windows")]
    {
        Err(color_eyre::eyre::eyre!("No defaut generator installed"))
            .wrap_err("cmake has not found any generator")
            .with_suggestion(|| "Install `Microsoft Visual Studio`")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn print_default_config() -> Result<(), Report> {
        let config = read_config()?;
        println!("{:?}", config.cmake_args());
        Ok(())
    }
}

