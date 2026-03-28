use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
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
    logs_buffer: Arc<Mutex<VecDeque<String>>>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IronclawLogs {
    content: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct LaunchEnvConfig {
    llm_backend: Option<String>,
    llm_base_url: Option<String>,
    llm_model: Option<String>,
    llm_api_key: Option<String>,
    openai_api_key: Option<String>,
    anthropic_api_key: Option<String>,
    nearai_api_key: Option<String>,
    ollama_base_url: Option<String>,
    feishu_app_id: Option<String>,
    feishu_app_secret: Option<String>,
    feishu_verification_token: Option<String>,
    telegram_bot_token: Option<String>,
    telegram_webhook_secret: Option<String>,
    slack_bot_token: Option<String>,
    slack_signing_secret: Option<String>,
    discord_bot_token: Option<String>,
    discord_public_key: Option<String>,
    whatsapp_access_token: Option<String>,
    whatsapp_verify_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelSavePayload {
    channel_type: String,
    app_id_or_token: Option<String>,
    app_secret: Option<String>,
    verification_token: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChannelSaveResult {
    channel_type: String,
    installed_files: Vec<String>,
    message: String,
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

    if let Some(path) = bundled_ironclaw_path(app) {
        if run_version_command_with_path(&path).is_some() {
            return Some(path);
        }
    }

    if run_version_command("ironclaw").is_some() {
        return Some(PathBuf::from("ironclaw"));
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

fn launch_env_store_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("读取配置目录失败: {e}"))?;
    path.push("launch_env.json");
    Ok(path)
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn normalize_launch_env_config(config: LaunchEnvConfig) -> LaunchEnvConfig {
    LaunchEnvConfig {
        llm_backend: normalize_optional(config.llm_backend),
        llm_base_url: normalize_optional(config.llm_base_url),
        llm_model: normalize_optional(config.llm_model),
        llm_api_key: normalize_optional(config.llm_api_key),
        openai_api_key: normalize_optional(config.openai_api_key),
        anthropic_api_key: normalize_optional(config.anthropic_api_key),
        nearai_api_key: normalize_optional(config.nearai_api_key),
        ollama_base_url: normalize_optional(config.ollama_base_url),
        feishu_app_id: normalize_optional(config.feishu_app_id),
        feishu_app_secret: normalize_optional(config.feishu_app_secret),
        feishu_verification_token: normalize_optional(config.feishu_verification_token),
        telegram_bot_token: normalize_optional(config.telegram_bot_token),
        telegram_webhook_secret: normalize_optional(config.telegram_webhook_secret),
        slack_bot_token: normalize_optional(config.slack_bot_token),
        slack_signing_secret: normalize_optional(config.slack_signing_secret),
        discord_bot_token: normalize_optional(config.discord_bot_token),
        discord_public_key: normalize_optional(config.discord_public_key),
        whatsapp_access_token: normalize_optional(config.whatsapp_access_token),
        whatsapp_verify_token: normalize_optional(config.whatsapp_verify_token),
    }
}

fn read_launch_env_config_from_store(app: &tauri::AppHandle) -> Option<LaunchEnvConfig> {
    let path = launch_env_store_path(app).ok()?;
    let content = fs::read_to_string(path).ok()?;
    let parsed = serde_json::from_str::<LaunchEnvConfig>(&content).ok()?;
    Some(normalize_launch_env_config(parsed))
}

fn write_launch_env_config_to_store(
    app: &tauri::AppHandle,
    config: &LaunchEnvConfig,
) -> Result<(), String> {
    let path = launch_env_store_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let content =
        serde_json::to_string_pretty(config).map_err(|e| format!("序列化模型配置失败: {e}"))?;
    fs::write(path, content).map_err(|e| format!("写入模型配置失败: {e}"))?;
    Ok(())
}

fn apply_launch_env_config(cmd: &mut Command, config: &LaunchEnvConfig) {
    if let Some(v) = config.llm_backend.as_deref() {
        cmd.env("LLM_BACKEND", v);
    }
    if let Some(v) = config.llm_base_url.as_deref() {
        cmd.env("LLM_BASE_URL", v);
    }
    if let Some(v) = config.llm_model.as_deref() {
        cmd.env("LLM_MODEL", v);
    }
    if let Some(v) = config.llm_api_key.as_deref() {
        cmd.env("LLM_API_KEY", v);
    }
    if let Some(v) = config.openai_api_key.as_deref() {
        cmd.env("OPENAI_API_KEY", v);
    }
    if let Some(v) = config.anthropic_api_key.as_deref() {
        cmd.env("ANTHROPIC_API_KEY", v);
    }
    if let Some(v) = config.nearai_api_key.as_deref() {
        cmd.env("NEARAI_API_KEY", v);
    }
    if let Some(v) = config.ollama_base_url.as_deref() {
        cmd.env("OLLAMA_BASE_URL", v);
    }
    if let Some(v) = config.feishu_app_id.as_deref() {
        cmd.env("FEISHU_APP_ID", v);
    }
    if let Some(v) = config.feishu_app_secret.as_deref() {
        cmd.env("FEISHU_APP_SECRET", v);
    }
    if let Some(v) = config.feishu_verification_token.as_deref() {
        cmd.env("FEISHU_VERIFICATION_TOKEN", v);
    }
    if let Some(v) = config.telegram_bot_token.as_deref() {
        cmd.env("TELEGRAM_BOT_TOKEN", v);
    }
    if let Some(v) = config.telegram_webhook_secret.as_deref() {
        cmd.env("TELEGRAM_WEBHOOK_SECRET", v);
    }
    if let Some(v) = config.slack_bot_token.as_deref() {
        cmd.env("SLACK_BOT_TOKEN", v);
    }
    if let Some(v) = config.slack_signing_secret.as_deref() {
        cmd.env("SLACK_SIGNING_SECRET", v);
    }
    if let Some(v) = config.discord_bot_token.as_deref() {
        cmd.env("DISCORD_BOT_TOKEN", v);
    }
    if let Some(v) = config.discord_public_key.as_deref() {
        cmd.env("DISCORD_PUBLIC_KEY", v);
    }
    if let Some(v) = config.whatsapp_access_token.as_deref() {
        cmd.env("WHATSAPP_ACCESS_TOKEN", v);
    }
    if let Some(v) = config.whatsapp_verify_token.as_deref() {
        cmd.env("WHATSAPP_VERIFY_TOKEN", v);
    }
}

fn channel_slug(channel_type: &str) -> Option<&'static str> {
    match channel_type.to_ascii_lowercase().as_str() {
        "feishu" => Some("feishu"),
        "telegram" => Some("telegram"),
        "slack" => Some("slack"),
        "discord" => Some("discord"),
        "whatsapp" => Some("whatsapp"),
        _ => None,
    }
}

fn bundled_channels_resource_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(mut path) = app.path().resource_dir() {
        path.push("channels");
        dirs.push(path);
    }
    let mut dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dev_path.push("resources");
    dev_path.push("channels");
    if !dirs.iter().any(|p| p == &dev_path) {
        dirs.push(dev_path);
    }
    dirs
}

fn ironclaw_channels_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .home_dir()
        .map_err(|e| format!("读取用户目录失败: {e}"))?;
    path.push(".ironclaw");
    path.push("channels");
    Ok(path)
}

fn ensure_channel_dm_policy(capabilities_path: &Path, _channel_slug: &str) -> Result<(), String> {
    let expected_policy = "open";
    let raw = fs::read_to_string(capabilities_path)
        .map_err(|e| format!("读取通道 capabilities 失败: {e}"))?;
    let mut json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("解析通道 capabilities 失败: {e}"))?;
    if let Some(config) = json.get_mut("config").and_then(serde_json::Value::as_object_mut) {
        config.insert(
            "dm_policy".to_string(),
            serde_json::Value::String(expected_policy.to_string()),
        );
    }
    let updated = serde_json::to_string_pretty(&json)
        .map_err(|e| format!("序列化通道 capabilities 失败: {e}"))?;
    fs::write(capabilities_path, updated)
        .map_err(|e| format!("写入通道 capabilities 失败: {e}"))?;
    Ok(())
}

fn install_bundled_channel(app: &tauri::AppHandle, channel_slug: &str) -> Result<Vec<String>, String> {
    let wasm_name = format!("{channel_slug}.wasm");
    let capabilities_name = format!("{channel_slug}.capabilities.json");
    let mut found_paths: Option<(PathBuf, PathBuf)> = None;
    let mut attempted_paths: Vec<(PathBuf, PathBuf)> = Vec::new();
    for resource_dir in bundled_channels_resource_dirs(app) {
        let source_wasm = resource_dir.join(&wasm_name);
        let source_capabilities = resource_dir.join(&capabilities_name);
        attempted_paths.push((source_wasm.clone(), source_capabilities.clone()));
        if source_wasm.is_file() && source_capabilities.is_file() {
            found_paths = Some((source_wasm, source_capabilities));
            break;
        }
    }
    let (source_wasm, source_capabilities) = found_paths.ok_or_else(|| {
        let attempted = attempted_paths
            .iter()
            .map(|(wasm, capabilities)| format!("{} 和 {}", wasm.display(), capabilities.display()))
            .collect::<Vec<_>>()
            .join("；");
        format!("未找到内置通道文件，请先执行构建通道资源。已尝试: {attempted}")
    })?;

    let target_dir = ironclaw_channels_dir(app)?;
    fs::create_dir_all(&target_dir).map_err(|e| format!("创建通道目录失败: {e}"))?;
    let target_wasm = target_dir.join(&wasm_name);
    let target_capabilities = target_dir.join(&capabilities_name);
    fs::copy(&source_wasm, &target_wasm).map_err(|e| format!("复制 wasm 失败: {e}"))?;
    fs::copy(&source_capabilities, &target_capabilities)
        .map_err(|e| format!("复制 capabilities 失败: {e}"))?;
    ensure_channel_dm_policy(&target_capabilities, channel_slug)?;

    Ok(vec![
        target_wasm.display().to_string(),
        target_capabilities.display().to_string(),
    ])
}

fn apply_channel_payload_to_env(config: &mut LaunchEnvConfig, channel_slug: &str, payload: &ChannelSavePayload) {
    let app_id_or_token = normalize_optional(payload.app_id_or_token.clone());
    let app_secret = normalize_optional(payload.app_secret.clone());
    let verification_token = normalize_optional(payload.verification_token.clone());
    match channel_slug {
        "feishu" => {
            config.feishu_app_id = app_id_or_token;
            config.feishu_app_secret = app_secret;
            config.feishu_verification_token = verification_token;
        }
        "telegram" => {
            config.telegram_bot_token = app_id_or_token;
            config.telegram_webhook_secret = verification_token;
        }
        "slack" => {
            config.slack_bot_token = app_id_or_token;
            config.slack_signing_secret = app_secret;
        }
        "discord" => {
            config.discord_bot_token = app_id_or_token;
            config.discord_public_key = app_secret;
        }
        "whatsapp" => {
            config.whatsapp_access_token = app_id_or_token;
            config.whatsapp_verify_token = verification_token;
        }
        _ => {}
    }
}

fn sanitize_log_line(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
                continue;
            }
            continue;
        }

        if ch.is_control() && ch != '\t' {
            continue;
        }

        let normalized = match ch {
            '╶' | '╴' | '─' | '━' | '│' | '┃' => '-',
            _ => ch,
        };
        out.push(normalized);
    }
    out
}

fn append_log_line(logs: &Arc<Mutex<VecDeque<String>>>, line: String) {
    if let Ok(mut guard) = logs.lock() {
        guard.push_back(line);
        while guard.len() > 2000 {
            guard.pop_front();
        }
    }
}

fn spawn_log_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    logs: Arc<Mutex<VecDeque<String>>>,
    stream: &'static str,
) {
    std::thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(content) => {
                    let normalized = sanitize_log_line(&content);
                    append_log_line(&logs, format!("[{stream}] {normalized}"));
                }
                Err(_) => break,
            }
        }
    });
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
    let patterns = ["ironclaw --no-onboard run", "ironclaw run"];
    let mut pids = Vec::new();
    for pattern in patterns {
        let output = Command::new("pgrep")
            .args(["-f", pattern])
            .output()
            .map_err(|e| format!("读取进程列表失败: {e}"))?;
        if !output.status.success() {
            continue;
        }
        pids.extend(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(|line| line.to_string())
                .collect::<Vec<_>>(),
        );
    }
    pids.sort();
    pids.dedup();
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

    if let Some(path) = bundled_candidate.clone() {
        let version = run_version_command_with_path(&path);
        return IronclawRuntimeInfo {
            configured_path: None,
            discovered_path: Some(path.display().to_string()),
            bundled_candidate_path: bundled_candidate.map(|p| p.display().to_string()),
            available: version.is_some(),
            version,
            message: "检测到安装包内置 ironclaw 候选路径".to_string(),
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
fn get_launch_env_config(app: tauri::AppHandle) -> LaunchEnvConfig {
    read_launch_env_config_from_store(&app).unwrap_or_default()
}

#[tauri::command]
fn set_launch_env_config(
    app: tauri::AppHandle,
    config: LaunchEnvConfig,
) -> Result<LaunchEnvConfig, String> {
    let normalized = normalize_launch_env_config(config);
    write_launch_env_config_to_store(&app, &normalized)?;
    Ok(normalized)
}

#[tauri::command]
fn save_channel_config(
    app: tauri::AppHandle,
    payload: ChannelSavePayload,
) -> Result<ChannelSaveResult, String> {
    let channel = channel_slug(&payload.channel_type)
        .ok_or_else(|| format!("不支持的通道类型: {}", payload.channel_type))?;
    let installed_files = install_bundled_channel(&app, channel)?;
    let mut config = read_launch_env_config_from_store(&app).unwrap_or_default();
    apply_channel_payload_to_env(&mut config, channel, &payload);
    let normalized = normalize_launch_env_config(config);
    write_launch_env_config_to_store(&app, &normalized)?;
    Ok(ChannelSaveResult {
        channel_type: channel.to_string(),
        installed_files,
        message: format!("通道已保存并安装到 ~/.ironclaw/channels: {channel}"),
    })
}

#[tauri::command]
fn get_ironclaw_logs(state: tauri::State<AppState>) -> Result<IronclawLogs, String> {
    let guard = state
        .logs_buffer
        .lock()
        .map_err(|_| "无法读取日志缓冲区".to_string())?;
    let content = guard.iter().cloned().collect::<Vec<_>>().join("\n");
    Ok(IronclawLogs { content })
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
    cmd.args(["--no-onboard", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.env("RUST_LOG", "ironclaw=debug,tower_http=warn");
    let gateway_auth_token = resolve_gateway_token(&app, &state)?.unwrap_or_default();
    cmd.env("GATEWAY_AUTH_TOKEN", gateway_auth_token);
    cmd.env("ONBOARD_COMPLETED", "1");
    if let Some(config) = read_launch_env_config_from_store(&app) {
        apply_launch_env_config(&mut cmd, &config);
    }

    let logs_buffer = Arc::clone(&state.logs_buffer);
    if let Ok(mut guard) = logs_buffer.lock() {
        guard.clear();
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 ironclaw run 失败: {e}"))?;
    let pid = child.id();
    append_log_line(
        &logs_buffer,
        format!("启动命令: ironclaw --no-onboard run (pid={pid})"),
    );
    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(stdout, Arc::clone(&logs_buffer), "stdout");
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(stderr, Arc::clone(&logs_buffer), "stderr");
    }
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
    append_log_line(
        &state.logs_buffer,
        format!("进程已停止: ironclaw --no-onboard run (pid={pid})"),
    );

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
            get_launch_env_config,
            set_launch_env_config,
            save_channel_config,
            get_ironclaw_logs,
            start_ironclaw_run,
            stop_ironclaw_run,
            get_ironclaw_run_status,
            open_console_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
