//! MSI installer builder for my-hub.
//!
//! 使用 cargo-packager 库（WiX Toolset 后端）生成 64 位 Windows Installer
//! (.msi) 安装包：安装 `my-hub.exe` 与默认 `manifest.toml`，注册开始菜单
//! 快捷方式与卸载信息。
//!
//! Usage:
//!   cargo build --release
//!   cargo run --release --bin build-installer --features installer
//!
//! 产物输出到 `target/wix/`（实际文件名以运行日志中 cargo-packager 的
//! PackageOutput 为准）。
//!
//! 说明：
//! - 首次打包时 cargo-packager 会自动下载并缓存 WiX Toolset，需要联网一次。
//! - 安装范围与旧版手写 MSI 保持一致（per-machine，写入 Program Files）。
//! - 版本号直接取本包 Cargo.toml 的 version（build-installer 与主程序
//!   同属 my-hub 包）。

#![cfg(windows)]

use anyhow::{bail, Context, Result};
use cargo_packager::{
    config::{Binary, Config, PackageFormat, Resource, WixConfig, WixLanguage},
    package,
};
use std::path::PathBuf;

const PRODUCT_NAME: &str = "my-hub";
const PUBLISHER: &str = "my-hub contributors";
/// 稳定标识符（反域名记法）。cargo-packager 据此派生 MSI 的升级码，
/// 保持跨构建稳定即可实现覆盖安装，而非越装越多。
const IDENTIFIER: &str = "com.bedrockmark.my-hub";
const DESCRIPTION: &str = "Central hub for the my-family suite";
const HOMEPAGE: &str = "https://github.com/BedrockMark/my-hub";

fn main() -> Result<()> {
    if let Err(e) = run() {
        eprintln!("build-installer error: {:#}", e);
        std::process::exit(1);
    }
    Ok(())
}

fn run() -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    // 用绝对路径（CARGO_MANIFEST_DIR 恒为绝对路径）：cargo-packager 会把
    // Binary 的相对路径按 Config::out_dir 解析，相对 target_dir 会错位。
    let target_dir = match std::env::var("CARGO_TARGET_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
    };

    // 定位已构建的 my-hub.exe
    let exe_src = target_dir.join("release").join(format!("{PRODUCT_NAME}.exe"));
    if !exe_src.exists() {
        bail!(
            "Could not find my-hub.exe at {}. Run `cargo build --release` first.",
            exe_src.display()
        );
    }
    // cargo-packager 的 Binary.path 要求不带 .exe 后缀
    let exe_stem = exe_src
        .parent()
        .expect("exe path has a parent")
        .join(exe_src.file_stem().expect("exe path has a file stem"));

    // 默认 manifest 必须存在（MSI 的固定载荷）；LICENSE 存在时附带为 license.txt
    let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_src = project_dir.join("assets/default-manifest.toml");
    if !manifest_src.exists() {
        bail!("Could not find {}.", manifest_src.display());
    }
    let license_src = project_dir.join("LICENSE");

    // cargo-packager 的 WiX 模板不重命名资源：安装名 = 源文件名，
    // Resource::Mapped 的 target 在该模板中不生效。旧版 MSI 的载荷名为
    // manifest.toml / license.txt，因此先把文件按目标名暂存到临时目录。
    let staging_dir =
        std::env::temp_dir().join(format!("my-hub-installer-{}", std::process::id()));
    std::fs::create_dir_all(&staging_dir)
        .with_context(|| format!("Failed to create {}", staging_dir.display()))?;

    // 资源安装到可执行文件同目录（cargo-packager 对 WiX 格式的行为）
    let mut resources = vec![Resource::Mapped {
        src: stage_as(&staging_dir, "manifest.toml", &manifest_src)?,
        target: "manifest.toml".into(),
    }];
    if license_src.exists() {
        resources.push(Resource::Mapped {
            src: stage_as(&staging_dir, "license.txt", &license_src)?,
            target: "license.txt".into(),
        });
    }

    let out_dir = target_dir.join("wix");
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("Failed to create {}", out_dir.display()))?;

    println!("Building MSI (cargo-packager / WiX)");
    println!("  product: {} {}", PRODUCT_NAME, version);
    println!("  binary: {} (main)", exe_src.display());
    println!(
        "  resources:\n    - {} -> manifest.toml",
        manifest_src.display()
    );
    if license_src.exists() {
        println!("    - {} -> license.txt", license_src.display());
    }
    println!("  out_dir: {}", out_dir.display());

    // 组装配置；LICENSE 是可选字段，不能在链上条件设置，故分两步
    let builder = Config::builder()
        .product_name(PRODUCT_NAME)
        .version(version)
        .identifier(IDENTIFIER)
        .publisher(PUBLISHER)
        .description(DESCRIPTION)
        .homepage(HOMEPAGE)
        .authors([PUBLISHER])
        .formats([PackageFormat::Wix])
        .out_dir(&out_dir)
        .binaries([Binary::new(exe_stem).main(true)])
        .resources(resources)
        .wix(WixConfig::new().languages([WixLanguage::Identifier("en-US".into())]));
    let builder = if license_src.exists() {
        builder.license_file(license_src)
    } else {
        builder
    };

    let result = package(builder.config()).context("cargo-packager packaging failed");
    // 无论成败都清理暂存目录
    let _ = std::fs::remove_dir_all(&staging_dir);
    let outputs = result?;
    for output in &outputs {
        for path in &output.paths {
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            println!("Done: {} ({} bytes)", path.display(), size);
        }
    }
    Ok(())
}

/// 把源文件复制到 staging 目录并命名为目标安装名，返回其字符串路径。
fn stage_as(staging_dir: &PathBuf, install_name: &str, src: &std::path::Path) -> Result<String> {
    let staged = staging_dir.join(install_name);
    std::fs::copy(src, &staged)
        .with_context(|| format!("Failed to stage {} as {}", src.display(), install_name))?;
    Ok(staged.to_string_lossy().into_owned())
}
