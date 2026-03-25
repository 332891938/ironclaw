use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::Manager;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IronclawRuntimeInfo {
    configured_path: Option<String>,
    discovered_path: Option<String>,
    bundled_candidate_path: Option<String>,
    version: Option<String>,
    available: bool,
    message: String,
}

#[derive(Default)]
struct AppState {
    ironclaw_child: Mutex<Option<Child>>,
    gateway_auth_token: Mutex<Option<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IronclawRunStatus {
    running: bool,
    pid: Option<u32>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayInfo {
    url: String,
    has_token: bool,
    token: Option<String>,
}

fn run_version_command(bin: &str) -> Option<String> {
    let output = Command::new(bin).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return None;
    }
    Some(stdout)
}

fn run_version_command_with_path(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return None;
    }
    Some(stdout)
}

fn bundled_ironclaw_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    let mut path = app.path().resource_dir().ok()?;
    path.push("bin");
    if cfg!(target_os = "windows") {
        path.push("ironclaw.exe");
    } else {
        path.push("ironclaw");
    }
    Some(path)
}

fn resolve_ironclaw_binary(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Some(configured_path) = env::var("IRONCLAW_BIN").ok() {
        let path = PathBuf::from(configured_path);
        if run_version_command_with_path(&path).is_some() {
            return Some(path);
        }
    }

    if run_version_command("ironclaw").is_some() {
        return Some(PathBuf::from("ironclaw"));
    }

    if let Some(path) = bundled_ironclaw_path(app) {
        if run_version_command_with_path(&path).is_some() {
            return Some(path);
        }
    }

    None
}

fn parse_env_value(raw: &str) -> String {
    let value = raw.trim();
    if value.len() >= 2 {
        if (value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\''))
        {
            return value[1..value.len() - 1].trim().to_string();
        }
    }
    value.to_string()
}

fn read_token_from_env_content(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let normalized = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let Some((key, value)) = normalized.split_once('=') else {
            continue;
        };

        if key.trim() == "GATEWAY_AUTH_TOKEN" {
            let parsed = parse_env_value(value);
            if !parsed.is_empty() {
                return Some(parsed);
            }
        }
    }
    None
}

fn find_dotenv_path() -> Option<PathBuf> {
    if let Ok(current_dir) = env::current_dir() {
        for dir in current_dir.ancestors() {
            let candidate = dir.join(".env");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            for dir in exe_dir.ancestors() {
                let candidate = dir.join(".env");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn read_gateway_auth_token_from_env() -> Option<String> {
    if let Ok(token) = env::var("GATEWAY_AUTH_TOKEN") {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let dotenv_path = find_dotenv_path()?;
    let content = fs::read_to_string(dotenv_path).ok()?;
    read_token_from_env_content(&content)
}

fn build_gateway_info(token: Option<String>) -> GatewayInfo {
    let has_token = token.as_ref().is_some_and(|value| !value.is_empty());
    let base = "http://127.0.0.1:3000/";
    let url = if let Some(token_value) = token.as_ref() {
        if !token_value.is_empty() {
            format!("{base}?token={token_value}")
        } else {
            base.to_string()
        }
    } else {
        base.to_string()
    };
    GatewayInfo {
        url,
        has_token,
        token,
    }
}

fn read_override_gateway_token(state: &tauri::State<AppState>) -> Result<Option<String>, String> {
    let guard = state
        .gateway_auth_token
        .lock()
        .map_err(|_| "无法读取网关令牌状态".to_string())?;
    Ok(guard.clone())
}

fn resolve_gateway_token(state: &tauri::State<AppState>) -> Result<Option<String>, String> {
    let override_token = read_override_gateway_token(state)?;
    if override_token.is_some() {
        return Ok(override_token);
    }
    Ok(read_gateway_auth_token_from_env())
}

#[tauri::command]
fn detect_ironclaw_runtime(app: tauri::AppHandle) -> IronclawRuntimeInfo {
    let configured = env::var("IRONCLAW_BIN").ok();
    let bundled_candidate = bundled_ironclaw_path(&app);

    if let Some(configured_path) = configured.clone() {
        let path = PathBuf::from(&configured_path);
        let version = run_version_command_with_path(&path);
        return IronclawRuntimeInfo {
            configured_path: Some(configured_path.clone()),
            discovered_path: Some(configured_path),
            bundled_candidate_path: bundled_candidate.map(|p| p.display().to_string()),
            available: version.is_some(),
            version,
            message: "优先使用 IRONCLAW_BIN 指定路径".to_string(),
        };
    }

    if let Some(version) = run_version_command("ironclaw") {
        return IronclawRuntimeInfo {
            configured_path: None,
            discovered_path: Some("ironclaw (PATH)".to_string()),
            bundled_candidate_path: bundled_candidate.map(|p| p.display().to_string()),
            available: true,
            version: Some(version),
            message: "检测到系统 PATH 中可用的 ironclaw".to_string(),
        };
    }

    if let Some(path) = bundled_candidate.clone() {
        let version = run_version_command_with_path(&path);
        return IronclawRuntimeInfo {
            configured_path: None,
            discovered_path: Some(path.display().to_string()),
            bundled_candidate_path: bundled_candidate.map(|p| p.display().to_string()),
            available: version.is_some(),
            version,
            message: "未检测到 PATH 版本，已检查内置候选路径".to_string(),
        };
    }

    IronclawRuntimeInfo {
        configured_path: None,
        discovered_path: None,
        bundled_candidate_path: None,
        available: false,
        version: None,
        message: "未找到可用的 ironclaw 可执行文件".to_string(),
    }
}

#[tauri::command]
fn get_gateway_info(state: tauri::State<AppState>) -> Result<GatewayInfo, String> {
    let token = resolve_gateway_token(&state)?;
    Ok(build_gateway_info(token))
}

#[tauri::command]
fn set_gateway_auth_token(state: tauri::State<AppState>, token: String) -> Result<GatewayInfo, String> {
    let normalized = token.trim().to_string();
    let next_value = if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    };

    {
        let mut guard = state
            .gateway_auth_token
            .lock()
            .map_err(|_| "无法更新网关令牌状态".to_string())?;
        *guard = next_value.clone();
    }

    if next_value.is_some() {
        Ok(build_gateway_info(next_value))
    } else {
        let fallback = read_gateway_auth_token_from_env();
        Ok(build_gateway_info(fallback))
    }
}

#[tauri::command]
fn start_ironclaw_run(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<IronclawRunStatus, String> {
    let mut child_guard = state
        .ironclaw_child
        .lock()
        .map_err(|_| "无法获取进程状态锁".to_string())?;

    if let Some(child) = child_guard.as_mut() {
        match child.try_wait() {
            Ok(Some(_)) => {
                *child_guard = None;
            }
            Ok(None) => {
                return Ok(IronclawRunStatus {
                    running: true,
                    pid: Some(child.id()),
                    message: "ironclaw run 已在运行".to_string(),
                });
            }
            Err(e) => {
                return Err(format!("检查进程状态失败: {e}"));
            }
        }
    }

    let bin = resolve_ironclaw_binary(&app)
        .ok_or_else(|| "未找到可用的 ironclaw 可执行文件".to_string())?;

    let mut cmd = Command::new(&bin);
    cmd.arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let gateway_auth_token = resolve_gateway_token(&state)?.unwrap_or_default();
    cmd.env("GATEWAY_AUTH_TOKEN", gateway_auth_token);

    let child = cmd
        .spawn()
        .map_err(|e| format!("启动 ironclaw run 失败: {e}"))?;
    let pid = child.id();
    *child_guard = Some(child);

    Ok(IronclawRunStatus {
        running: true,
        pid: Some(pid),
        message: format!("已启动 ironclaw run (pid={pid})"),
    })
}

#[tauri::command]
fn stop_ironclaw_run(state: tauri::State<AppState>) -> Result<IronclawRunStatus, String> {
    let mut child_guard = state
        .ironclaw_child
        .lock()
        .map_err(|_| "无法获取进程状态锁".to_string())?;

    let Some(mut child) = child_guard.take() else {
        return Ok(IronclawRunStatus {
            running: false,
            pid: None,
            message: "当前没有由客户端启动的 ironclaw run 进程".to_string(),
        });
    };

    let pid = child.id();
    child
        .kill()
        .map_err(|e| format!("停止进程失败 (pid={pid}): {e}"))?;
    let _ = child.wait();

    Ok(IronclawRunStatus {
        running: false,
        pid: None,
        message: format!("已停止 ironclaw run 进程 (pid={pid})"),
    })
}

#[tauri::command]
fn get_ironclaw_run_status(state: tauri::State<AppState>) -> Result<IronclawRunStatus, String> {
    let mut child_guard = state
        .ironclaw_child
        .lock()
        .map_err(|_| "无法获取进程状态锁".to_string())?;

    if let Some(child) = child_guard.as_mut() {
        match child.try_wait() {
            Ok(Some(_)) => {
                *child_guard = None;
                Ok(IronclawRunStatus {
                    running: false,
                    pid: None,
                    message: "ironclaw run 已退出".to_string(),
                })
            }
            Ok(None) => Ok(IronclawRunStatus {
                running: true,
                pid: Some(child.id()),
                message: "ironclaw run 运行中".to_string(),
            }),
            Err(e) => Err(format!("读取进程状态失败: {e}")),
        }
    } else {
        Ok(IronclawRunStatus {
            running: false,
            pid: None,
            message: "当前没有由客户端启动的 ironclaw run 进程".to_string(),
        })
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            detect_ironclaw_runtime,
            get_gateway_info,
            set_gateway_auth_token,
            start_ironclaw_run,
            stop_ironclaw_run,
            get_ironclaw_run_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
