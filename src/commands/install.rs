use super::llvm::{install_version, llvm_16, llvm_17, llvm_18};
use crate::{Args, InstallSubcommand};
use color_eyre::eyre::Report;

#[derive(Debug)]
pub(crate) enum InstallError {}

pub(crate) async fn run(_: &Args, install: &InstallSubcommand) -> Result<(), Report> {
    match (install.name.as_str(), install.version.as_str()) {
        ("llvm", "16") => llvm_16().await,
        ("llvm", "17") => llvm_17().await,
        ("llvm", "18") => llvm_18().await,
        ("llvm", "19") => install_version("19.1.0", "LLVM_SYS_190_PREFIX").await,
        _ => todo!(),
    }
}
