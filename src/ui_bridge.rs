//! Bridge between the Slint UI and the Rust app state.

use anyhow::{Context, Result};
use slint::{ComponentHandle, VecModel};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

use crate::app_state::AppState;
use crate::config::theme::ResolvedTheme;
use crate::github::GitHubClient;
use crate::update::state::{DownloadedVersion, InstalledVersion};
use crate::update::{Downloader, ProgramUpdateInfo, UpdateChecker};

slint::include_modules!();

pub fn run(state: AppState) -> Result<()> {
    let ui = MainWindow::new()?;
    let state = Arc::new(Mutex::new(state));
    let runtime = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;

    // Push initial theme
    {
        let s = state.lock().unwrap();
        let theme = ResolvedTheme::resolve(&s.settings, dark_mode(&s.settings.theme));
        ui.set_theme(slint_theme(&theme));
    }

    // Push initial settings to UI
    {
        let s = state.lock().unwrap();
        ui.set_check_on_startup(s.settings.check_on_startup);
        ui.set_download_dir(s.settings.download_dir.clone().into());
        ui.set_move_to_exe_dir(s.settings.move_to_exe_dir);
        ui.set_github_pat(s.settings.github_pat.clone().unwrap_or_default().into());
        ui.set_theme_mode(s.settings.theme.clone().into());
        ui.set_accent_color(s.settings.custom_colors.accent.clone().into());
        ui.set_background_color(s.settings.custom_colors.background.clone().into());
        ui.set_text_color(s.settings.custom_colors.text.clone().into());
        if let Some(last) = &s.update_state.last_check {
            ui.set_last_check(last.clone().into());
        }
    }
    // 以注册表真实状态为准同步开机自启开关
    {
        let exe = crate::platform::current_exe_path().unwrap_or_default();
        let enabled = crate::platform::is_autostart_enabled(&exe).unwrap_or(false);
        ui.set_start_with_windows(enabled);
    }

    refresh_program_list(&ui, &state.lock().unwrap(), None);

    // Startup auto-check (per user settings)
    let auto_check = {
        let s = state.lock().unwrap();
        s.settings.check_on_startup
    };
    if auto_check {
        info!("Startup update check enabled - checking now");
        spawn_check(&ui, &state, runtime.handle());
    }

    // ---- Wire callbacks ----

    let ui_check = ui.as_weak();
    let state_check = Arc::clone(&state);
    let runtime_check = runtime.handle().clone();
    ui.on_check_now(move || {
        info!("Check now requested");
        let _ = ui_check.upgrade_in_event_loop(move |ui| {
            ui.set_is_checking(true);
        });
        spawn_check_handle(&ui_check, &state_check, &runtime_check, true);
    });

    let ui_weak = ui.as_weak();
    let state_for_open_settings = Arc::clone(&state);
    ui.on_open_settings(move || {
        info!("Open settings");
        let value = state_for_open_settings.clone();
        let _ = ui_weak.upgrade_in_event_loop(move |ui| {
            let s = value.lock().unwrap();
            ui.set_check_on_startup(s.settings.check_on_startup);
            ui.set_start_with_windows(s.settings.start_with_windows);
            ui.set_download_dir(s.settings.download_dir.clone().into());
            ui.set_move_to_exe_dir(s.settings.move_to_exe_dir);
            ui.set_github_pat(s.settings.github_pat.clone().unwrap_or_default().into());
            ui.set_theme_mode(s.settings.theme.clone().into());
            ui.set_accent_color(s.settings.custom_colors.accent.clone().into());
            ui.set_background_color(s.settings.custom_colors.background.clone().into());
            ui.set_text_color(s.settings.custom_colors.text.clone().into());
            ui.set_settings_visible(true);
        });
    });

    let ui_weak = ui.as_weak();
    ui.on_close_settings(move || {
        let _ = ui_weak.upgrade_in_event_loop(|ui| {
            ui.set_settings_visible(false);
        });
    });

    let state_for_save_gen = Arc::clone(&state);
    let ui_for_save_gen = ui.as_weak();
    ui.on_save_general(move |check, start_windows, move_to_exe_dir, download_dir, github_pat| {
        info!("Save general settings");
        let mut s = state_for_save_gen.lock().unwrap();
        s.settings.check_on_startup = check;
        s.settings.start_with_windows = start_windows;
        s.settings.move_to_exe_dir = move_to_exe_dir;
        s.settings.download_dir = download_dir.to_string();
        s.settings.github_pat = if github_pat.is_empty() {
            None
        } else {
            Some(github_pat.to_string())
        };
        if let Err(e) = s.save() {
            warn!("Failed to save settings: {:#}", e);
        }
        let exe = crate::platform::current_exe_path().unwrap_or_default();
        if let Err(e) = crate::platform::set_autostart(start_windows, &exe) {
            warn!("Failed to set autostart: {:#}", e);
        }
        let _ = ui_for_save_gen.upgrade_in_event_loop(|ui| {
            ui.set_settings_visible(false);
        });
    });

    let state_for_save_theme = Arc::clone(&state);
    let ui_for_save_theme = ui.as_weak();
    ui.on_save_theme(move |mode, accent, bg, text| {
        info!("Save theme settings");
        let mut s = state_for_save_theme.lock().unwrap();
        s.settings.theme = mode.to_string();
        s.settings.custom_colors.accent = accent.to_string();
        s.settings.custom_colors.background = bg.to_string();
        s.settings.custom_colors.text = text.to_string();
        if let Err(e) = s.save() {
            warn!("Failed to save settings: {:#}", e);
        }
        let theme = ResolvedTheme::resolve(&s.settings, dark_mode(&s.settings.theme));
        let _ = ui_for_save_theme.upgrade_in_event_loop(move |ui| {
            ui.set_theme(slint_theme(&theme));
        });
    });

    let state_for_update = Arc::clone(&state);
    let ui_for_update = ui.as_weak();
    let runtime_for_update = runtime.handle().clone();
    ui.on_request_update(move |id| {
        info!("Request update for {}", id);
        // 提取 program 条目 + 上次检查缓存的 release 信息（UI 线程内，快速）
        let (program, release, download_dir, move_to_exe_dir) = {
            let s = state_for_update.lock().unwrap();
            let p = match s.manifest.find(id.as_str()) {
                Some(p) => p.clone(),
                None => {
                    warn!("Request update for unknown program id {}", id);
                    return;
                }
            };
            let rel = s.check_results.get(id.as_str()).and_then(|r| r.release.clone());
            (p, rel, s.settings.download_dir.clone(), s.settings.move_to_exe_dir)
        };
        let Some(release) = release else {
            warn!("No cached release info for {} - run a check first", id);
            return;
        };
        let Some(asset) = release.assets.iter().find(|a| a.name == program.executable) else {
            warn!(
                "Release of {} has no asset matching executable '{}'",
                id, program.executable
            );
            return;
        };

        let ui_w = ui_for_update.clone();
        let state_rc = Arc::clone(&state_for_update);
        let is_self = program.id == "my-hub";
        let url = asset.browser_download_url.clone();
        let asset_name = asset.name.clone();
        let program_id = program.id.clone();
        let tag = release.tag_name.clone();

        // 按设置决定下载目标目录：勾选 move-to-exe-dir 时直接下载到 exe 同目录 installed 子文件夹
        let target_dir = match resolve_download_target(move_to_exe_dir, &download_dir) {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to resolve download target for {}: {:#}", id, e);
                return;
            }
        };

        // 供 async 任务内部重检使用（FnMut 闭包环境中无法被 move 捕获）
        let runtime_handle = runtime_for_update.clone();

        runtime_for_update.spawn(async move {
            let downloader = match Downloader::new(target_dir) {
                Ok(d) => d,
                Err(e) => {
                    warn!("Failed to init downloader: {:#}", e);
                    return;
                }
            };
            match downloader.download(&url, &asset_name).await {
                Ok(path) => {
                    info!("Downloaded {} to {}", asset_name, path.display());
                    if is_self {
                        // 自更新：stage 新 exe，延迟替换并重启
                        match apply_self_update(&path) {
                            Ok(()) => {
                                // 记录新版本，重启后 UI 显示 current 版本
                                if let Ok(mut s) = state_rc.lock() {
                                    record_installed_version(&mut s, &program_id, &tag);
                                    if let Err(e) = s.save() {
                                        warn!("Failed to persist version after self-update: {:#}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Self-update failed: {:#}", e);
                            }
                        }
                    } else {
                        // 非自身程序：记录下载位置到状态（MVP：仅记录，不自动替换）
                        if let Ok(mut s) = state_rc.lock() {
                            s.update_state.downloaded_versions.insert(
                                program_id.clone(),
                                DownloadedVersion {
                                    version: tag.clone(),
                                    path: path.to_string_lossy().to_string(),
                                },
                            );
                            // 下载完成即视为已安装该版本：保存版本号供 UI 显示
                            record_installed_version(&mut s, &program_id, &tag);
                            if let Err(e) = s.save() {
                                warn!("Failed to persist download record: {:#}", e);
                            }
                        }
                    }
                    // 自动静默重检：确认是否为最新版（不弹更新对话框），完成后刷新列表
                    spawn_check_handle(&ui_w, &state_rc, &runtime_handle, false);
                    let _ = ui_w.upgrade_in_event_loop(move |ui| {
                        // 关闭更新对话框
                        let mut data = ui.get_dialog_data();
                        data.program_id = "".into();
                        ui.set_dialog_data(data);
                        ui.set_is_checking(false);
                    });
                }
                Err(e) => {
                    warn!("Download failed for {}: {:#}", program_id, e);
                    let _ = ui_w.upgrade_in_event_loop(move |ui| {
                        ui.set_is_checking(false);
                    });
                }
            }
        });
    });

    let state_for_skip = Arc::clone(&state);
    let ui_for_skip = ui.as_weak();
    ui.on_skip_version(move |id, version| {
        info!("Skip version {} for {}", version, id);
        {
            let mut s = state_for_skip.lock().unwrap();
            s.update_state.skip_versions.insert(id.to_string(), version.to_string());
            let _ = s.save();
        }
        let value = state_for_skip.clone();
        let _ = ui_for_skip.upgrade_in_event_loop(move |ui| {
            // 关闭更新对话框
            let mut data = ui.get_dialog_data();
            data.program_id = "".into();
            ui.set_dialog_data(data);
            ui.set_is_checking(false);
            let st = value.lock().unwrap();
            refresh_program_list(&ui, &st, None);
        });
    });

    let state_for_ignore = Arc::clone(&state);
    let ui_for_ignore = ui.as_weak();
    ui.on_ignore_program(move |id| {
        info!("Ignore program {}", id);
        {
            let mut s = state_for_ignore.lock().unwrap();
            if !s.update_state.ignored_programs.contains(&id.to_string()) {
                s.update_state.ignored_programs.push(id.to_string());
                let _ = s.save();
            }
        }
        let value = state_for_ignore.clone();
        let _ = ui_for_ignore.upgrade_in_event_loop(move |ui| {
            // 关闭更新对话框
            let mut data = ui.get_dialog_data();
            data.program_id = "".into();
            ui.set_dialog_data(data);
            // 用新的 ignore 状态刷新列表
            let st = value.lock().unwrap();
            refresh_program_list(&ui, &st, None);
        });
    });

    let state_for_launch = Arc::clone(&state);
    ui.on_launch_program(move |id| {
        info!("Launch program {}", id);
        let s = state_for_launch.lock().unwrap();
        if let Some(program) = s.manifest.find(id.as_str()) {
            launch_program(&s, program);
        }
    });

    let state_for_uninstall = Arc::clone(&state);
    let ui_for_uninstall = ui.as_weak();
    ui.on_uninstall_program(move |id| {
        info!("Uninstall program {}", id);
        {
            let mut s = state_for_uninstall.lock().unwrap();
            let removed_installed = s.update_state.installed_versions.remove(id.as_str()).is_some();
            let removed_downloaded = s.update_state.downloaded_versions.remove(id.as_str()).is_some();
            if removed_installed || removed_downloaded {
                if let Err(e) = s.save() {
                    warn!("Failed to persist state after uninstall: {:#}", e);
                }
            } else {
                warn!("No installed record for {} - nothing to uninstall", id);
            }
        }
        let value = state_for_uninstall.clone();
        let _ = ui_for_uninstall.upgrade_in_event_loop(move |ui| {
            let st = value.lock().unwrap();
            refresh_program_list(&ui, &st, None);
        });
    });

    let ui_for_close_dialog = ui.as_weak();
    ui.on_close_dialog(move || {
        let _ = ui_for_close_dialog.upgrade_in_event_loop(|ui| {
            let mut data = ui.get_dialog_data();
            data.program_id = "".into();
            ui.set_dialog_data(data);
        });
    });

    info!("UI ready, starting event loop");
    ui.run()?;
    Ok(())
}

/// Spawn a background update check. The actual GitHub queries run on the tokio
/// runtime; results are applied back to the UI via the event loop.
fn spawn_check(ui: &MainWindow, state: &Arc<Mutex<AppState>>, handle: &tokio::runtime::Handle) {
    let _ = ui.as_weak().upgrade_in_event_loop(|ui| {
        ui.set_is_checking(true);
    });
    spawn_check_handle(&ui.as_weak(), state, handle, true);
}

/// Shared helper that launches the async check task.
/// `pop_dialog` 为 true 时检查完成后会弹出更新对话框；false 用于下载/更新后的静默重检。
fn spawn_check_handle(
    ui_weak: &slint::Weak<MainWindow>,
    state: &Arc<Mutex<AppState>>,
    handle: &tokio::runtime::Handle,
    pop_dialog: bool,
) {
    // 提取后台任务需要的数据（均为 Send）
    let (manifest, pat, update_state) = {
        let s = state.lock().unwrap();
        (
            s.manifest.clone(),
            s.settings.github_pat.clone(),
            s.update_state.clone(),
        )
    };
    let ui_w = ui_weak.clone();
    let state_rc = Arc::clone(state);

    handle.spawn(async move {
        let results = match GitHubClient::new(pat) {
            Ok(client) => {
                let checker = UpdateChecker::new(&manifest, &update_state, &client);
                checker.check_all().await
            }
            Err(e) => {
                warn!("Failed to build GitHub client: {:#}", e);
                Vec::new()
            }
        };
        let _ = ui_w.upgrade_in_event_loop(move |ui| {
            apply_check_results(&ui, &state_rc, &results, pop_dialog);
        });
    });
}

/// Apply check results on the UI thread: cache results, refresh the list,
/// optionally pop the update dialog for the first relevant update, reset checking state.
fn apply_check_results(
    ui: &MainWindow,
    state: &Arc<Mutex<AppState>>,
    results: &[ProgramUpdateInfo],
    pop_dialog: bool,
) {
    // 1. 缓存结果 + 更新 last_check
    {
        let mut s = state.lock().unwrap();
        s.check_results.clear();
        for r in results {
            s.check_results.insert(r.program_id.clone(), r.clone());
        }
        s.update_state.last_check = Some(chrono::Utc::now().to_rfc3339());
        if let Err(e) = s.save() {
            warn!("Failed to persist state: {:#}", e);
        }
        if let Some(last) = &s.update_state.last_check {
            ui.set_last_check(last.clone().into());
        }
    }

    // 2. 刷新程序列表
    refresh_program_list(ui, &state.lock().unwrap(), Some(results));

    // 3. 弹出第一个有更新的程序对话框（跳过 ignored / 已跳过该版本）——仅用户主动检查时
    if pop_dialog {
        let dialog = {
            let s = state.lock().unwrap();
            results
                .iter()
                .find(|r| {
                    r.has_update
                        && !r.ignored
                        && !s.update_state.is_version_skipped(
                            &r.program_id,
                            r.latest_version.as_deref().unwrap_or_default(),
                        )
                })
                .and_then(|r| {
                    let p = s.manifest.find(&r.program_id)?;
                    let rel = r.release.as_ref()?;
                    Some((
                        r.program_id.clone(),
                        p.name.clone(),
                        r.current_version.clone().unwrap_or_default(),
                        r.latest_version.clone().unwrap_or_default(),
                        rel.published_at.clone(),
                        rel.body.clone(),
                        r.ignored,
                    ))
                })
        };
        if let Some((pid, name, cur, latest, published, notes, ignored)) = dialog {
            let mut data = ui.get_dialog_data();
            data.program_id = pid.into();
            data.program_name = name.into();
            data.current_version = cur.into();
            data.latest_version = latest.into();
            data.published_at = published.into();
            data.release_notes = notes.into();
            data.ignored = ignored;
            ui.set_dialog_data(data);
        }
    }

    ui.set_is_checking(false);
}

/// Rebuild the program list model. When `results` is provided, latest-version /
/// update flags come from the last check instead of placeholders.
fn refresh_program_list(
    ui: &MainWindow,
    state: &AppState,
    results: Option<&[ProgramUpdateInfo]>,
) {
    let rows: Vec<ProgramInfo> = state
        .manifest
        .program
        .iter()
        .map(|p| {
            let installed = state.update_state.installed_versions.get(&p.id);
            let current = installed.map(|v| v.version.clone()).unwrap_or_default();
            let ignored = state.update_state.is_ignored(&p.id);
            // 优先用本次检查结果；若未提供（如卸载/跳过后的刷新），回退到内存缓存的检查结果
            let check = results
                .and_then(|rs| rs.iter().find(|r| r.program_id == p.id))
                .or_else(|| state.check_results.get(&p.id));
            let latest = check
                .and_then(|c| c.latest_version.clone())
                .unwrap_or_default();
            let has_update = check.map(|c| c.has_update).unwrap_or(false);
            // 已安装且启用，但拿不到成功的检查结果（尚未检查 / 检查失败）→ UI 显示 ERROR
            let check_failed = !current.is_empty()
                && p.enabled
                && check.map(|c| c.latest_version.is_none()).unwrap_or(true);
            // 跳过已标记忽略版本的更新提示
            let skipped = check
                .map(|c| {
                    c.has_update
                        && state.update_state.is_version_skipped(
                            &p.id,
                            c.latest_version.as_deref().unwrap_or_default(),
                        )
                })
                .unwrap_or(false);
            let downloaded = state.update_state.downloaded_versions.get(&p.id);
            ProgramInfo {
                id: p.id.clone().into(),
                name: p.name.clone().into(),
                description: p.description.clone().into(),
                owner: p.owner.clone().into(),
                repo: p.repo.clone().into(),
                current_version: current.clone().into(),
                latest_version: latest.clone().into(),
                has_update: has_update && !skipped,
                check_failed,
                ignored,
                enabled: p.enabled,
                is_self: p.id == "my-hub",
                status_text: if ignored {
                    "Ignored".into()
                } else if !p.enabled {
                    "Disabled".into()
                } else if downloaded.is_some() {
                    "Downloaded".into()
                } else if current.is_empty() {
                    "Not installed".into()
                } else if has_update && !skipped {
                    "Update available".into()
                } else {
                    "Up to date".into()
                },
            }
        })
        .collect();
    ui.set_programs(Rc::new(VecModel::from(rows)).into());
}

/// Launch a program via the OS default application (native launch):
/// resolve the program file location, then open it with opener.
fn launch_program(state: &AppState, entry: &crate::config::manifest::ProgramEntry) {
    let path = resolve_program_path(state, entry);
    if let Err(e) = opener::open(&path) {
        warn!("Failed to launch {} ({}): {:#}", entry.id, path.display(), e);
    }
}

/// 定位程序可执行文件路径（launch directory 下的文件）：优先已下载记录（含最终位置），
/// 否则按设置推断所在目录（installed 子文件夹或下载目录）。
fn resolve_program_path(state: &AppState, entry: &crate::config::manifest::ProgramEntry) -> PathBuf {
    if let Some(d) = state.update_state.downloaded_versions.get(&entry.id) {
        return PathBuf::from(&d.path);
    }
    if state.settings.move_to_exe_dir {
        if let Ok(exe) = crate::platform::current_exe_path() {
            if let Some(dir) = exe.parent() {
                return dir.join("installed").join(&entry.executable);
            }
        }
    }
    PathBuf::from(&state.settings.download_dir).join(&entry.executable)
}

/// 解析下载目标目录：move_to_exe_dir 时直接返回 exe 同目录下的 "installed" 子文件夹（自动创建），
/// 否则返回下载目录。
fn resolve_download_target(move_to_exe_dir: bool, download_dir: &str) -> anyhow::Result<PathBuf> {
    if move_to_exe_dir {
        let exe = crate::platform::current_exe_path()?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Failed to resolve exe parent directory"))?;
        let dir = exe_dir.join("installed");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create install dir {}", dir.display()))?;
        Ok(dir)
    } else {
        Ok(PathBuf::from(download_dir))
    }
}

/// 把某程序的最新检查版本记为已安装版本（供 UI 显示 current 版本）。
/// 优先使用检查结果中的 latest_version（无 'v' 前缀，保证 semver 比较一致），
/// 拿不到时回退到 tag 本身。
fn record_installed_version(s: &mut AppState, program_id: &str, fallback_tag: &str) {
    let version = s
        .check_results
        .get(program_id)
        .and_then(|r| r.latest_version.clone())
        .unwrap_or_else(|| fallback_tag.trim_start_matches('v').to_string());
    s.update_state.installed_versions.insert(
        program_id.to_string(),
        InstalledVersion {
            version,
            installed_at: chrono::Utc::now().to_rfc3339(),
        },
    );
}

fn dark_mode(theme_mode: &str) -> bool {
    match theme_mode {
        "Dark" => true,
        "Light" => false,
        _ => true,
    }
}

fn slint_theme(t: &ResolvedTheme) -> Theme {
    Theme {
        background: parse_hex(&t.background),
        surface: parse_hex(&t.surface),
        surface_alt: parse_hex(&t.surface_alt),
        border: parse_hex(&t.border),
        text: parse_hex(&t.text),
        text_muted: parse_hex(&t.text_muted),
        accent: parse_hex(&t.accent),
        accent_hover: parse_hex(&t.accent),
        success: parse_hex(&t.success),
        warning: parse_hex(&t.warning),
        danger: parse_hex("#ef4444"),
    }
}

fn parse_hex(hex: &str) -> slint::Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return slint::Color::from_rgb_u8(128, 128, 128);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(128);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(128);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(128);
    slint::Color::from_rgb_u8(r, g, b)
}

/// Self-update: stage the freshly downloaded exe next to the running one,
/// write a small batch script that swaps it in after this process exits,
/// launch the script and terminate the app.
fn apply_self_update(new_exe: &std::path::Path) -> Result<()> {
    let current = std::env::current_exe().context("Failed to get current exe path")?;
    let dir = current
        .parent()
        .context("Failed to resolve current exe directory")?;
    let staged = dir.join("my-hub-new.exe");
    std::fs::copy(new_exe, &staged).context("Failed to stage new executable")?;

    let bat = dir.join("my-hub-update.bat");
    let script = format!(
        "@echo off\r\n\
         timeout /t 2 /nobreak >nul\r\n\
         move /y \"{}\" \"{}\"\r\n\
         start \"\" \"{}\"\r\n\
         del \"%~f0\"\r\n",
        staged.display(),
        current.display(),
        current.display()
    );
    std::fs::write(&bat, script).context("Failed to write update script")?;
    std::process::Command::new("cmd")
        .arg("/c")
        .arg(&bat)
        .spawn()
        .context("Failed to launch update script")?;
    info!("Self-update staged - restarting to apply");
    std::process::exit(0);
}
