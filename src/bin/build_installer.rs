//! MSI installer builder for my-hub.
//!
//! Generates a per-user Windows Installer (.msi) package that installs
//! `my-hub.exe` plus the default `manifest.toml`, registers a Start Menu
//! shortcut, and registers uninstall information.
//!
//! Usage:
//!   cargo build --release
//!   cargo run --release --bin build-installer
//!
//! Output: `target/wix/my-hub-<version>-x86_64.msi`

#![cfg(windows)]

use anyhow::{Context, Result};
use msi::{Category, CodePage, Column, Insert, Package, PackageType, Value};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

const PRODUCT_NAME: &str = "my-hub";
const MANUFACTURER: &str = "my-hub contributors";
const UPGRADE_CODE: &str = "{00000000-0000-0000-0000-000000000001}";

fn main() -> Result<()> {
    if let Err(e) = run() {
        eprintln!("build-installer error: {:#}", e);
        std::process::exit(1);
    }
    Ok(())
}

fn run() -> Result<()> {
    let manifest = read_cargo_manifest().context("Failed to read Cargo.toml")?;
    let version = manifest_version(&manifest);
    let out_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());
    let out_path = PathBuf::from(&out_dir)
        .join("wix")
        .join(format!("my-hub-{}-x86_64.msi", version));
    std::fs::create_dir_all(out_path.parent().unwrap())
        .with_context(|| format!("Failed to create {}", out_path.parent().unwrap().display()))?;

    // Locate my-hub.exe
    let exe_src = PathBuf::from(&out_dir)
        .join("release")
        .join(format!("{}.exe", PRODUCT_NAME));
    if !exe_src.exists() {
        anyhow::bail!(
            "Could not find my-hub.exe at {}. Run `cargo build --release` first.",
            exe_src.display()
        );
    }
    let exe_data = std::fs::read(&exe_src)
        .with_context(|| format!("Failed to read {}", exe_src.display()))?;

    // Locate default manifest
    let manifest_src = Path::new("assets/default-manifest.toml");
    let manifest_data = std::fs::read(manifest_src)
        .with_context(|| format!("Failed to read {}", manifest_src.display()))?;

    // Locate LICENSE if present
    let license_src = Path::new("LICENSE");
    let license_data: Option<Vec<u8>> = if license_src.exists() {
        Some(
            std::fs::read(license_src)
                .with_context(|| format!("Failed to read {}", license_src.display()))?,
        )
    } else {
        None
    };

    println!("Building MSI: {}", out_path.display());
    println!("  exe: {} ({} bytes)", exe_src.display(), exe_data.len());
    println!(
        "  manifest: {} ({} bytes)",
        manifest_src.display(),
        manifest_data.len()
    );
    if let Some(ref l) = license_data {
        println!("  license: {} ({} bytes)", license_src.display(), l.len());
    }

    build_msi(
        &out_path,
        &version,
        &exe_data,
        &manifest_data,
        license_data.as_deref(),
    )?;

    let size = std::fs::metadata(&out_path)?.len();
    println!("Done: {} ({} bytes)", out_path.display(), size);
    Ok(())
}

fn build_msi(
    out_path: &Path,
    version: &str,
    exe_data: &[u8],
    manifest_data: &[u8],
    license_data: Option<&[u8]>,
) -> Result<()> {
    let cursor = Cursor::new(Vec::new());
    let mut package = Package::create(PackageType::Installer, cursor)
        .context("Failed to create MSI package")?;

    package.set_database_codepage(CodePage::Utf8);

    {
        let info = package.summary_info_mut();
        info.set_title(PRODUCT_NAME.to_string());
        info.set_subject(PRODUCT_NAME.to_string());
        info.set_author(MANUFACTURER.to_string());
        info.set_keywords(&["Installer".to_string()]);
        info.set_comments(format!("{} version {}", PRODUCT_NAME, version));
        info.set_creating_application(format!("my-hub build-installer {}", version));
    }

    create_property_table(&mut package, version)?;
    create_directory_table(&mut package)?;
    create_component_table(&mut package)?;
    create_feature_table(&mut package)?;
    create_feature_components_table(&mut package)?;
    create_file_table(&mut package, exe_data, manifest_data, license_data)?;
    create_media_table(&mut package)?;
    create_install_execute_sequence_table(&mut package)?;
    create_install_ui_sequence_table(&mut package)?;
    create_shortcut_table(&mut package)?;
    create_upgrade_table(&mut package)?;
    create_launch_condition_table(&mut package)?;

    write_stream(&mut package, "my-hub.exe", exe_data)?;
    write_stream(&mut package, "manifest.toml", manifest_data)?;
    if let Some(license) = license_data {
        write_stream(&mut package, "license.txt", license)?;
    }

    let cursor = package
        .into_inner()
        .context("Failed to finalize MSI package")?;
    let bytes = cursor.into_inner();
    std::fs::write(out_path, bytes)
        .with_context(|| format!("Failed to write MSI to {}", out_path.display()))?;

    Ok(())
}

fn write_stream(package: &mut Package<Cursor<Vec<u8>>>, name: &str, data: &[u8]) -> Result<()> {
    let mut writer = package
        .write_stream(name)
        .with_context(|| format!("Failed to open stream writer for {}", name))?;
    writer
        .write_all(data)
        .with_context(|| format!("Failed to write stream {}", name))?;
    Ok(())
}

fn create_property_table(package: &mut Package<Cursor<Vec<u8>>>, version: &str) -> Result<()> {
    let columns = vec![
        Column::build("Property").primary_key().id_string(72),
        Column::build("Value").nullable().text_string(255),
    ];
    package
        .create_table("Property", columns)
        .context("Failed to create Property table")?;

    let entries: Vec<(&str, String)> = vec![
        ("ProductName", PRODUCT_NAME.into()),
        ("ProductVersion", version.to_string()),
        ("ProductCode", generate_guid()),
        ("UpgradeCode", UPGRADE_CODE.into()),
        ("Manufacturer", MANUFACTURER.into()),
        ("ProductLanguage", "1033".into()),
        ("ALLUSERS", "2".into()),
        ("MSIINSTALLPERUSER", "1".into()),
        ("ARPNOMODIFY", "1".into()),
        ("ARPNOREPAIR", "1".into()),
        ("InstallPerUser", "1".into()),
        ("ALLUSERS_NOT_SET", "1".into()),
        ("Privileged", "0".into()),
        ("DefaultUIFont", "DlgFont8".into()),
        ("ErrorDialog", "ErrorDlg".into()),
        ("ProgressDlg0", "Progress0".into()),
        ("ProgressDlg1", "Progress1".into()),
    ];
    for (k, v) in entries {
        let row = vec![Value::from(k), Value::from(v)];
        package
            .insert_rows(Insert::into("Property").row(row))
            .context("Failed to insert Property row")?;
    }
    Ok(())
}

fn create_directory_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("Directory").primary_key().id_string(72),
        Column::build("Directory_Parent").nullable().id_string(72),
        Column::build("DefaultDir").nullable().string(255),
    ];
    package
        .create_table("Directory", columns)
        .context("Failed to create Directory table")?;

    let rows: Vec<(Value, Value, Value)> = vec![
        ("TARGETDIR".into(), Value::Null, "SourceDir".into()),
        ("ProgramFilesFolder".into(), "TARGETDIR".into(), Value::Null),
        ("LocalAppDataFolder".into(), "TARGETDIR".into(), Value::Null),
        ("AppDataFolder".into(), "TARGETDIR".into(), Value::Null),
        ("INSTALLDIR".into(), "ProgramFilesFolder".into(), "my-hub".into()),
        ("AppDataDir".into(), "AppDataFolder".into(), "my-hub".into()),
        ("StartMenuFolder".into(), "TARGETDIR".into(), "StartMenu".into()),
        ("MyHUBStartMenu".into(), "StartMenuFolder".into(), "my-hub".into()),
    ];
    for (dir, parent, default) in rows {
        let row = vec![dir, parent, default];
        package
            .insert_rows(Insert::into("Directory").row(row))
            .context("Failed to insert Directory row")?;
    }
    Ok(())
}

fn create_component_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("Component").primary_key().id_string(72),
        Column::build("ComponentId")
            .nullable()
            .category(Category::Guid)
            .string(38),
        Column::build("Directory_").nullable().id_string(72),
        Column::build("Attributes").int32(),
        Column::build("Condition").nullable().string(255),
        Column::build("KeyPath").nullable().category(Category::Identifier).string(72),
    ];
    package
        .create_table("Component", columns)
        .context("Failed to create Component table")?;

    let rows: Vec<Vec<Value>> = vec![
        vec![
            "MainExecutable".into(),
            Value::from(generate_guid()),
            "INSTALLDIR".into(),
            Value::Int(0),
            Value::Null,
            Value::Null,
        ],
        vec![
            "DefaultManifest".into(),
            Value::from(generate_guid()),
            "INSTALLDIR".into(),
            Value::Int(0),
            Value::Null,
            Value::Null,
        ],
        vec![
            "StartMenuShortcut".into(),
            Value::from(generate_guid()),
            "MyHUBStartMenu".into(),
            Value::Int(0),
            Value::Null,
            Value::Null,
        ],
    ];
    for row in rows {
        package
            .insert_rows(Insert::into("Component").row(row))
            .context("Failed to insert Component row")?;
    }
    Ok(())
}

fn create_feature_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("Feature").primary_key().id_string(38),
        Column::build("Feature_Parent").nullable().id_string(38),
        Column::build("Title").nullable().string(64),
        Column::build("Description").nullable().string(255),
        Column::build("Display").nullable().int32(),
        Column::build("Level").int32(),
        Column::build("Directory_").nullable().id_string(72),
        Column::build("Attributes").int32(),
    ];
    package
        .create_table("Feature", columns)
        .context("Failed to create Feature table")?;

    let row = vec![
        Value::from("Complete"),
        Value::Null,
        Value::from("my-hub"),
        Value::from("Install all of my-hub"),
        Value::Int(0),
        Value::Int(1),
        Value::from("INSTALLDIR"),
        Value::Int(0),
    ];
    package
        .insert_rows(Insert::into("Feature").row(row))
        .context("Failed to insert Feature row")?;
    Ok(())
}

fn create_feature_components_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("Feature_").primary_key().id_string(38),
        Column::build("Component_").primary_key().id_string(72),
    ];
    package
        .create_table("FeatureComponents", columns)
        .context("Failed to create FeatureComponents table")?;

    let rows: Vec<(Value, Value)> = vec![
        ("Complete".into(), "MainExecutable".into()),
        ("Complete".into(), "DefaultManifest".into()),
        ("Complete".into(), "StartMenuShortcut".into()),
    ];
    for (feat, comp) in rows {
        let row = vec![feat, comp];
        package
            .insert_rows(Insert::into("FeatureComponents").row(row))
            .context("Failed to insert FeatureComponents row")?;
    }
    Ok(())
}

fn create_file_table(
    package: &mut Package<Cursor<Vec<u8>>>,
    exe_data: &[u8],
    manifest_data: &[u8],
    license_data: Option<&[u8]>,
) -> Result<()> {
    let columns = vec![
        Column::build("File")
            .primary_key()
            .category(Category::Identifier)
            .string(72),
        Column::build("Component_").id_string(72),
        Column::build("FileName").string(255),
        Column::build("Size").int32(),
        Column::build("Version").nullable().string(72),
        Column::build("Language").nullable().string(20),
        Column::build("Attributes").int32(),
        Column::build("Sequence").int32(),
    ];
    package
        .create_table("File", columns)
        .context("Failed to create File table")?;

    let mut sequence = 1i32;
    let mut rows: Vec<Vec<Value>> = vec![
        vec![
            "myhub_exe".into(),
            "MainExecutable".into(),
            "my-hub.exe".into(),
            Value::Int(exe_data.len() as i32),
            Value::Null,
            Value::Null,
            Value::Int(16384),
            Value::Int(sequence),
        ],
        vec![
            "manifest_toml".into(),
            "DefaultManifest".into(),
            "manifest.toml".into(),
            Value::Int(manifest_data.len() as i32),
            Value::Null,
            Value::Null,
            Value::Int(16384),
            Value::Int(sequence + 1),
        ],
    ];
    sequence += 2;

    if let Some(license) = license_data {
        rows.push(vec![
            "license_txt".into(),
            "DefaultManifest".into(),
            "license.txt".into(),
            Value::Int(license.len() as i32),
            Value::Null,
            Value::Null,
            Value::Int(16384),
            Value::Int(sequence),
        ]);
    }

    for row in rows {
        package
            .insert_rows(Insert::into("File").row(row))
            .context("Failed to insert File row")?;
    }
    Ok(())
}

fn create_media_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("Media").primary_key().int32(),
        Column::build("LastSequence").int32(),
        Column::build("DiskPrompt").nullable().string(64),
        Column::build("Cabinet").nullable().string(255),
        Column::build("VolumeLabel").nullable().string(32),
        Column::build("Source").nullable().string(72),
    ];
    package
        .create_table("Media", columns)
        .context("Failed to create Media table")?;

    let row = vec![
        Value::Int(1),
        Value::Int(3),
        Value::Null,
        Value::Null,
        Value::Null,
        Value::from(PRODUCT_NAME),
    ];
    package
        .insert_rows(Insert::into("Media").row(row))
        .context("Failed to insert Media row")?;
    Ok(())
}

fn create_install_execute_sequence_table(
    package: &mut Package<Cursor<Vec<u8>>>,
) -> Result<()> {
    let columns = vec![
        Column::build("Action").primary_key().id_string(72),
        Column::build("Condition").nullable().string(255),
        Column::build("Sequence").nullable().int32(),
    ];
    package
        .create_table("InstallExecuteSequence", columns)
        .context("Failed to create InstallExecuteSequence table")?;

    let actions: Vec<(&str, Option<&str>, i32)> = vec![
        ("LaunchConditions", None, 100),
        ("AppSearch", None, 400),
        ("ValidateProductID", None, 700),
        ("CostInitialize", None, 800),
        ("FileCost", None, 900),
        ("CostFinalize", None, 1000),
        ("InstallValidate", None, 1100),
        ("InstallInitialize", None, 1200),
        ("InstallFiles", None, 1300),
        ("CreateShortcuts", None, 1400),
        ("RegisterProduct", None, 1500),
        ("PublishFeatures", None, 1600),
        ("PublishProduct", None, 1700),
        ("InstallFinalize", None, 2000),
        ("RemoveFiles", None, 3000),
    ];
    for (action, condition, sequence) in actions {
        let row = vec![
            Value::from(action),
            condition.map(Value::from).unwrap_or(Value::Null),
            Value::Int(sequence),
        ];
        package
            .insert_rows(Insert::into("InstallExecuteSequence").row(row))
            .context("Failed to insert InstallExecuteSequence row")?;
    }
    Ok(())
}

fn create_install_ui_sequence_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("Action").primary_key().id_string(72),
        Column::build("Condition").nullable().string(255),
        Column::build("Sequence").nullable().int32(),
    ];
    package
        .create_table("InstallUISequence", columns)
        .context("Failed to create InstallUISequence table")?;

    let actions: Vec<(&str, Option<&str>, i32)> = vec![
        ("LaunchConditions", None, 100),
        ("AppSearch", None, 400),
        ("CostInitialize", None, 800),
        ("FileCost", None, 900),
        ("CostFinalize", None, 1000),
        ("ExecuteAction", None, 1300),
    ];
    for (action, condition, sequence) in actions {
        let row = vec![
            Value::from(action),
            condition.map(Value::from).unwrap_or(Value::Null),
            Value::Int(sequence),
        ];
        package
            .insert_rows(Insert::into("InstallUISequence").row(row))
            .context("Failed to insert InstallUISequence row")?;
    }
    Ok(())
}

fn create_shortcut_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("Shortcut")
            .primary_key()
            .category(Category::Identifier)
            .string(72),
        Column::build("Directory_").id_string(72),
        Column::build("Name").string(255),
        Column::build("Component_").id_string(72),
        Column::build("Target").id_string(72),
        Column::build("Arguments").nullable().string(255),
        Column::build("Description").nullable().string(255),
        Column::build("Hotkey").nullable().int32(),
        Column::build("Icon_").nullable().id_string(72),
        Column::build("IconIndex").nullable().int32(),
        Column::build("ShowCmd").nullable().int32(),
        Column::build("WkDir").nullable().id_string(72),
    ];
    package
        .create_table("Shortcut", columns)
        .context("Failed to create Shortcut table")?;

    let row = vec![
        Value::from("myhub_lnk"),
        Value::from("MyHUBStartMenu"),
        Value::from("my-hub"),
        Value::from("StartMenuShortcut"),
        Value::from("myhub_exe"),
        Value::Null,
        Value::from("Central hub for the my-family suite"),
        Value::Null,
        Value::Null,
        Value::Null,
        Value::Int(1),
        Value::from("INSTALLDIR"),
    ];
    package
        .insert_rows(Insert::into("Shortcut").row(row))
        .context("Failed to insert Shortcut row")?;
    Ok(())
}

fn create_upgrade_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("UpgradeCode")
            .primary_key()
            .category(Category::Guid)
            .string(38),
        Column::build("VersionMin").nullable().string(20),
        Column::build("VersionMax").nullable().string(20),
        Column::build("Language").nullable().string(20),
        Column::build("Attributes").int32(),
        Column::build("Remove").nullable().string(255),
        Column::build("ActionProperty").nullable().id_string(72),
    ];
    package
        .create_table("Upgrade", columns)
        .context("Failed to create Upgrade table")?;

    let row = vec![
        Value::from(UPGRADE_CODE),
        Value::Null,
        Value::Null,
        Value::Null,
        Value::Int(257),
        Value::Null,
        Value::from("OLDER_VERSION_FOUND"),
    ];
    package
        .insert_rows(Insert::into("Upgrade").row(row))
        .context("Failed to insert Upgrade row")?;
    Ok(())
}

fn create_launch_condition_table(package: &mut Package<Cursor<Vec<u8>>>) -> Result<()> {
    let columns = vec![
        Column::build("Condition").primary_key().string(255),
        Column::build("Description").string(255),
    ];
    package
        .create_table("LaunchCondition", columns)
        .context("Failed to create LaunchCondition table")?;

    let row = vec![
        Value::from("VersionNT"),
        Value::from("my-hub requires Windows NT or later"),
    ];
    package
        .insert_rows(Insert::into("LaunchCondition").row(row))
        .context("Failed to insert LaunchCondition row")?;
    Ok(())
}

fn generate_guid() -> String {
    // MSI GUID format: uppercase, hyphenated, enclosed in braces
    format!("{{{}}}", uuid::Uuid::new_v4().hyphenated().to_string().to_uppercase())
}

fn read_cargo_manifest() -> Result<toml::Value> {
    let text = std::fs::read_to_string("Cargo.toml").context("Failed to read Cargo.toml")?;
    let value: toml::Value = toml::from_str(&text).context("Failed to parse Cargo.toml")?;
    Ok(value)
}

fn manifest_version(manifest: &toml::Value) -> String {
    manifest
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("0.1.0")
        .to_string()
}