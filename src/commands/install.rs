use super::llvm::install_version;
use crate::{Args, InstallSubcommand};
use color_eyre::eyre::Report;

#[derive(Debug)]
pub(crate) enum InstallError {}

pub(crate) async fn run(_: &Args, install: &InstallSubcommand) -> Result<(), Report> {
    match (install.name.as_str(), install.version.as_str()) {
        ("llvm", "10") => install_version("10.0.1", "LLVM_SYS_100_PREFIX").await,
        ("llvm", "11") => install_version("11.1.0", "LLVM_SYS_110_PREFIX").await,
        ("llvm", "12") => install_version("12.0.1", "LLVM_SYS_120_PREFIX").await,
        ("llvm", "13") => install_version("13.0.1", "LLVM_SYS_130_PREFIX").await,
        ("llvm", "14") => install_version("14.0.6", "LLVM_SYS_140_PREFIX").await,
        ("llvm", "15") => install_version("15.0.7", "LLVM_SYS_150_PREFIX").await,
        ("llvm", "16") => install_version("16.0.5", "LLVM_SYS_160_PREFIX").await,
        ("llvm", "17") => install_version("17.0.6", "LLVM_SYS_170_PREFIX").await,
        ("llvm", "18") => install_version("18.1.2", "LLVM_SYS_180_PREFIX").await,
        ("llvm", "19") => install_version("19.1.0", "LLVM_SYS_190_PREFIX").await,
        _ => todo!(),
    }
}

