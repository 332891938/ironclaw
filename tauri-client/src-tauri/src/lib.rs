use serde::Serialize;
use std::env;
use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
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

const POSTMESSAGE_ONLY_INVOKE_SYSTEM: &str = r#"
;(function () {
  const __TAURI_INVOKE_KEY__ = __INVOKE_KEY__
  function processIpcMessage(message) {
    if (
      message instanceof ArrayBuffer
      || ArrayBuffer.isView(message)
      || Array.isArray(message)
    ) {
      return {
        contentType: 'application/octet-stream',
        data: message
      }
    } else {
      const data = JSON.stringify(message, (_k, val) => {
        const SERIALIZE_TO_IPC_FN = '__TAURI_TO_IPC_KEY__'
        if (val instanceof Map) {
          return Object.fromEntries(val.entries())
        } else if (val instanceof Uint8Array) {
          return Array.from(val)
        } else if (val instanceof ArrayBuffer) {
          return Array.from(new Uint8Array(val))
        } else if (
          typeof val === 'object'
          && val !== null
          && SERIALIZE_TO_IPC_FN in val
        ) {
          return val[SERIALIZE_TO_IPC_FN]()
        } else {
          return val
        }
      })
      return {
        contentType: 'application/json',
        data
      }
    }
  }
  Object.defineProperty(window.__TAURI_INTERNALS__, 'postMessage', {
    value: Object.freeze((message) => {
      const payload = {
        ...message,
        options: {
          ...(message.options || {}),
          customProtocolIpcBlocked: true
        },
        __TAURI_INVOKE_KEY__
      }
      const { data } = processIpcMessage(payload)
      window.ipc.postMessage(data)
    })
  })
})()
"#;

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

fn gateway_token_store_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("读取配置目录失败: {e}"))?;
    path.push("gateway_auth_token");
    Ok(path)
}

fn read_gateway_auth_token_from_store(app: &tauri::AppHandle) -> Option<String> {
    let path = gateway_token_store_path(app).ok()?;
    let content = fs::read_to_string(path).ok()?;
    let token = content.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn write_gateway_auth_token_to_store(
    app: &tauri::AppHandle,
    token: Option<&str>,
) -> Result<(), String> {
    let path = gateway_token_store_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    if let Some(value) = token {
        fs::write(path, value).map_err(|e| format!("写入 token 失败: {e}"))?;
    } else if path.exists() {
        fs::remove_file(path).map_err(|e| format!("清理 token 失败: {e}"))?;
    }
    Ok(())
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

fn detect_external_gateway_running() -> bool {
    let Ok(addr) = "127.0.0.1:3000".parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
}

#[cfg(target_os = "windows")]
fn stop_external_ironclaw_run() -> Result<bool, String> {
    let output = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq ironclaw.exe"])
        .output()
        .map_err(|e| format!("读取进程列表失败: {e}"))?;
    let listing = String::from_utf8_lossy(&output.stdout).to_lowercase();
    if !listing.contains("ironclaw.exe") {
        return Ok(false);
    }
    let status = Command::new("taskkill")
        .args(["/F", "/T", "/IM", "ironclaw.exe"])
        .status()
        .map_err(|e| format!("执行 taskkill 失败: {e}"))?;
    if !status.success() {
        return Err("taskkill 执行失败".to_string());
    }
    Ok(true)
}

#[cfg(not(target_os = "windows"))]
fn stop_external_ironclaw_run() -> Result<bool, String> {
    let output = Command::new("pgrep")
        .args(["-f", "ironclaw run"])
        .output()
        .map_err(|e| format!("读取进程列表失败: {e}"))?;
    if !output.status.success() {
        return Ok(false);
    }
    let pids = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    if pids.is_empty() {
        return Ok(false);
    }
    for pid in pids {
        let status = Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .map_err(|e| format!("执行 kill 失败: {e}"))?;
        if !status.success() {
            return Err(format!("kill 执行失败: pid={pid}"));
        }
    }
    Ok(true)
}

fn read_override_gateway_token(state: &tauri::State<AppState>) -> Result<Option<String>, String> {
    let guard = state
        .gateway_auth_token
        .lock()
        .map_err(|_| "无法读取网关令牌状态".to_string())?;
    Ok(guard.clone())
}

fn resolve_gateway_token(
    app: &tauri::AppHandle,
    state: &tauri::State<AppState>,
) -> Result<Option<String>, String> {
    let override_token = read_override_gateway_token(state)?;
    if override_token.is_some() {
        return Ok(override_token);
    }
    if let Some(stored_token) = read_gateway_auth_token_from_store(app) {
        return Ok(Some(stored_token));
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
fn get_gateway_info(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<GatewayInfo, String> {
    let token = resolve_gateway_token(&app, &state)?;
    Ok(build_gateway_info(token))
}

#[tauri::command]
fn set_gateway_auth_token(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    token: String,
) -> Result<GatewayInfo, String> {
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

    write_gateway_auth_token_to_store(&app, next_value.as_deref())?;

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

    if detect_external_gateway_running() {
        return Ok(IronclawRunStatus {
            running: true,
            pid: None,
            message: "检测到已有 ironclaw run 在运行".to_string(),
        });
    }

    let bin = resolve_ironclaw_binary(&app)
        .ok_or_else(|| "未找到可用的 ironclaw 可执行文件".to_string())?;

    let mut cmd = Command::new(&bin);
    cmd.arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let gateway_auth_token = resolve_gateway_token(&app, &state)?.unwrap_or_default();
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
        if detect_external_gateway_running() {
            let killed = stop_external_ironclaw_run()?;
            if killed {
                return Ok(IronclawRunStatus {
                    running: false,
                    pid: None,
                    message: "已停止外部启动的 ironclaw run 进程".to_string(),
                });
            }
            return Ok(IronclawRunStatus {
                running: true,
                pid: None,
                message: "检测到外部启动的 ironclaw run，但未匹配到可停止的进程".to_string(),
            });
        }
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
                if detect_external_gateway_running() {
                    Ok(IronclawRunStatus {
                        running: true,
                        pid: None,
                        message: "检测到外部启动的 ironclaw run 正在运行".to_string(),
                    })
                } else {
                    Ok(IronclawRunStatus {
                        running: false,
                        pid: None,
                        message: "ironclaw run 已退出".to_string(),
                    })
                }
            }
            Ok(None) => Ok(IronclawRunStatus {
                running: true,
                pid: Some(child.id()),
                message: "ironclaw run 运行中".to_string(),
            }),
            Err(e) => Err(format!("读取进程状态失败: {e}")),
        }
    } else {
        if detect_external_gateway_running() {
            Ok(IronclawRunStatus {
                running: true,
                pid: None,
                message: "检测到外部启动的 ironclaw run 正在运行".to_string(),
            })
        } else {
            Ok(IronclawRunStatus {
                running: false,
                pid: None,
                message: "当前没有由客户端启动的 ironclaw run 进程".to_string(),
            })
        }
    }
}

#[tauri::command]
fn open_console_window(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed_url = url
        .parse()
        .map_err(|e| format!("控制台地址无效: {e}"))?;

    if let Some(existing) = app.get_webview_window("console-window") {
        let _ = existing.close();
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        "console-window",
        tauri::WebviewUrl::External(parsed_url),
    )
    .title("IronClaw 控制台")
    .inner_size(1400.0, 900.0)
    .min_inner_size(1100.0, 700.0)
    .focused(true)
    .center()
    .build()
    .map_err(|e| format!("创建控制台窗口失败: {e}"))?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_opener::init())
        .invoke_system(POSTMESSAGE_ONLY_INVOKE_SYSTEM)
        .invoke_handler(tauri::generate_handler![
            detect_ironclaw_runtime,
            get_gateway_info,
            set_gateway_auth_token,
            start_ironclaw_run,
            stop_ironclaw_run,
            get_ironclaw_run_status,
            open_console_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
