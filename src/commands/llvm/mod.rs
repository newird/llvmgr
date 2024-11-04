use super::{
    add_line_to_file, cache_path, cache_root, download_ungz_untar, download_unxz_untar,
    get_cmake_default_generator, init_config, mkdir_inside_cache_folder, move_dir, read_config,
    read_shell, remove_dir, search_cmake, set_current_dir_inside_cache_folder, spawn_cmake,
    write_shell,
};
use crate::tasks::Tasks;
use color_eyre::{
    eyre::WrapErr,
    eyre::{ContextCompat, Report},
    Help,
};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};
use tokio::io::{self, AsyncBufReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};

pub fn download_url(version: &str) -> (String, String) {
    (
        format!("https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-{version}.tar.gz"),
        format!("llvmorg-{version}.tar.gz"),
    )
}
pub async fn install_version(version: &str, env_var: &str) -> Result<(), Report> {
    let config = read_config().wrap_err("can't read config")?;
    let version_root_folder = mkdir_inside_cache_folder(version)?;
    let llvm_source_code_folder = mkdir_inside_cache_folder(format!("{version}/src"))?;

    let (source_code_url, source_code_filename) = download_url(version);

    let mut tasks = Tasks::new();

    let t0 = tasks
        .new_task(source_code_filename.as_str())
        .wrap_err("Cannot report progress")?;
    let t1 = tasks
        .new_task("Compilation")
        .wrap_err("Cannot report progress")?;
    let t2 = tasks
        .new_task("Installation")
        .wrap_err("Cannot report progress")?;
    let t3 = tasks
        .new_task("Configuring shell")
        .wrap_err("Cannot report progress")?;

    let t4 = tasks.new_task("Clean").wrap_err("Cannot report progress")?;
    let _ = std::fs::remove_dir_all(&version_root_folder);

    //Download and uncompress source code
    //205541214 bytes
    let llvm_tar_gz_file_path = download_ungz_untar(&t0, source_code_url, llvm_source_code_folder)
        .await
        .wrap_err("Downloading source code")?;
    if let Some(value) = config.cache.get("delete_xz") {
        if value == "true" {
            t0.set_subtask("Cleaning downloaded files...");
            let _ = std::fs::remove_file(llvm_tar_gz_file_path);
        }
    }

    t0.finish();
    // Compilation
    set_current_dir_inside_cache_folder(format!("{version}/src/"))?;
    let big_version: i32 = version
        .split('.')
        .next()
        .expect("Version string is empty")
        .parse::<i32>()
        .expect("Failed to parse version part to i32");

    if big_version <= 12 {
        //  src/llvm/include/llvm/Support/Signals.h:18:1: note: ‘uintptr_t’ is defined in header ‘<cstdint>’; this is probably fixable by adding ‘#include <cstdint>’
        //  17 | #include <string>
        //  +++ |+#include <cstdint>
        let path_need_add_header =
            cache_path(format!("{version}/src/llvm/include/llvm/Support/Signals.h"))?;
        add_line_to_file(&path_need_add_header, "#include <cstdint>".to_string(), 18)?;
    }
    spawn_cmake(&t1, config.cmake_args())?;
    spawn_cmake(&t1, ["--build", "build"])?;
    t1.finish();

    t2.finish();
    // Setup env vars
    t3.set_subtask("configuring shell");
    let mut shell = read_shell()?;
    let var = shell.env_vars.entry(env_var.into()).or_default();
    *var = mkdir_inside_cache_folder(version)?.display().to_string();
    write_shell(&shell)?;
    t3.finish();
    t4.set_subtask("bin");
    move_dir(
        cache_path(version)?,
    )?;

    t4.set_subtask("lib");
    move_dir(
        cache_path(version)?,
    )?;

    t4.set_subtask("include");
    move_dir(
        cache_path(version)?,
    )?;
    if let Some(rm_src) = config.cache.get("delete_src") {
        if rm_src == "true" {
            t4.set_subtask("remove source");
            remove_dir(cache_path(format!("{version}/src/"))?)?;
        }
    }
    Ok(())
}

