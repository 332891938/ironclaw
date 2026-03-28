use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Cursor};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

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
    tunnel_child: Mutex<Option<Child>>,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolSavePayload {
    tool_name: String,
    install_source: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolSaveResult {
    tool_name: String,
    installed_files: Vec<String>,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillSavePayload {
    install_source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillSaveResult {
    skill_name: String,
    installed_files: Vec<String>,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TunnelSavePayload {
    username: String,
    password: String,
    node_id: Option<String>,
    channel_type: String,
    verification_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TunnelSaveResult {
    username: String,
    node_id: String,
    channel_slug: String,
    command: String,
    workdir: String,
    config_path: String,
    binary_path: String,
    tunnel_url: String,
    tunnel_pid: u32,
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
    #[cfg(target_os = "linux")]
    path.push("linux-amd64");
    #[cfg(target_os = "macos")]
    path.push("macos-arm64");
    #[cfg(target_os = "windows")]
    path.push("windows-amd64");
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

fn tunnel_channel_slug(channel_type: &str) -> &'static str {
    match channel_type.to_ascii_lowercase().as_str() {
        "telegram" => "telegram",
        "slack" => "slack",
        "discord" => "discord",
        "whatsapp" => "whatsapp",
        _ => "feishu",
    }
}

fn tunnel_binary_name() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "tunnel-client-darwin-arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "tunnel-client-darwin-amd64"
    }
    #[cfg(target_os = "linux")]
    {
        "tunnel-client-linux-amd64"
    }
    #[cfg(target_os = "windows")]
    {
        "tunnel-client-windows-amd64.exe"
    }
}

fn bundled_tunnel_resource_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(mut path) = app.path().resource_dir() {
        path.push("tunnel");
        dirs.push(path);
    }
    let mut dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dev_path.push("resources");
    dev_path.push("tunnel");
    if !dirs.iter().any(|p| p == &dev_path) {
        dirs.push(dev_path);
    }
    dirs
}

fn tunnel_runtime_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .home_dir()
        .map_err(|e| format!("读取用户目录失败: {e}"))?;
    path.push(".ironclaw");
    path.push("tunnel");
    Ok(path)
}

fn generate_tunnel_node_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = u128::from(std::process::id());
    let mix = nanos ^ (pid << 64) ^ (nanos.rotate_left(17));
    format!("{:032x}{:016x}", mix, nanos & 0xffff_ffff_ffff_ffff)
}

fn update_workbot_yaml_field_in_section(content: &str, section: &str, key: &str, value: &str) -> String {
    let mut updated = Vec::new();
    let mut current_section: Option<String> = None;
    let mut replaced_in_target = false;
    let target_prefix = format!("  {key}:");

    for line in content.lines() {
        let trimmed_end = line.trim_end();
        if !trimmed_end.starts_with(' ') && trimmed_end.ends_with(':') {
            let section_name = trimmed_end.trim_end_matches(':').trim().to_string();
            current_section = Some(section_name);
        }

        if line.starts_with(&target_prefix) {
            if current_section.as_deref() == Some(section) {
                if !replaced_in_target {
                    updated.push(format!("  {key}: \"{value}\""));
                    replaced_in_target = true;
                }
                continue;
            }
            continue;
        }

        updated.push(line.to_string());
    }

    if !replaced_in_target {
        let mut inserted = false;
        let mut with_insert = Vec::new();
        let mut current_section: Option<String> = None;

        for line in &updated {
            let trimmed_end = line.trim_end();
            if !trimmed_end.starts_with(' ') && trimmed_end.ends_with(':') {
                if current_section.as_deref() == Some(section) && !inserted {
                    with_insert.push(format!("  {key}: \"{value}\""));
                    inserted = true;
                }
                let section_name = trimmed_end.trim_end_matches(':').trim().to_string();
                current_section = Some(section_name);
            }
            with_insert.push(line.clone());
        }

        if current_section.as_deref() == Some(section) && !inserted {
            with_insert.push(format!("  {key}: \"{value}\""));
            inserted = true;
        }

        if inserted {
            return with_insert.join("\n");
        }
    }

    updated.join("\n")
}

fn ensure_tunnel_runtime_assets(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let runtime_dir = tunnel_runtime_dir(app)?;
    fs::create_dir_all(&runtime_dir).map_err(|e| format!("创建 tunnel 目录失败: {e}"))?;
    let binary_name = tunnel_binary_name();
    let config_name = "workbot.yaml";
    let mut found_binary: Option<PathBuf> = None;
    let mut found_config: Option<PathBuf> = None;
    for resource_dir in bundled_tunnel_resource_dirs(app) {
        if found_binary.is_none() {
            let candidate = resource_dir.join(binary_name);
            if candidate.is_file() {
                found_binary = Some(candidate);
            }
        }
        if found_config.is_none() {
            let candidate = resource_dir.join(config_name);
            if candidate.is_file() {
                found_config = Some(candidate);
            }
        }
    }
    let source_binary = found_binary.ok_or_else(|| format!("未找到内置 tunnel 客户端: {binary_name}"))?;
    let source_config = found_config.ok_or_else(|| "未找到内置 tunnel 配置: workbot.yaml".to_string())?;
    let target_binary = runtime_dir.join(binary_name);
    let target_config = runtime_dir.join(config_name);
    fs::copy(&source_binary, &target_binary).map_err(|e| format!("复制 tunnel 客户端失败: {e}"))?;
    fs::copy(&source_config, &target_config).map_err(|e| format!("复制 workbot.yaml 失败: {e}"))?;
    if cfg!(not(target_os = "windows")) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&target_binary)
                .map_err(|e| format!("读取 tunnel 客户端权限失败: {e}"))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&target_binary, perms)
                .map_err(|e| format!("设置 tunnel 客户端权限失败: {e}"))?;
        }
    }
    Ok((target_binary, target_config))
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

fn ironclaw_tools_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .home_dir()
        .map_err(|e| format!("读取用户目录失败: {e}"))?;
    path.push(".ironclaw");
    path.push("tools");
    Ok(path)
}

fn ironclaw_skills_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .home_dir()
        .map_err(|e| format!("读取用户目录失败: {e}"))?;
    path.push(".ironclaw");
    path.push("skills");
    Ok(path)
}

fn ironclaw_installed_skills_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .home_dir()
        .map_err(|e| format!("读取用户目录失败: {e}"))?;
    path.push(".ironclaw");
    path.push("installed_skills");
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

fn bundled_tools_resource_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(mut path) = app.path().resource_dir() {
        path.push("tools");
        dirs.push(path);
    }
    let mut dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dev_path.push("resources");
    dev_path.push("tools");
    if !dirs.iter().any(|p| p == &dev_path) {
        dirs.push(dev_path);
    }
    dirs
}

fn bundled_skills_resource_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(mut path) = app.path().resource_dir() {
        path.push("skills");
        dirs.push(path);
    }
    let mut dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dev_path.push("resources");
    dev_path.push("skills");
    if !dirs.iter().any(|p| p == &dev_path) {
        dirs.push(dev_path);
    }
    dirs
}

fn install_bundled_tool(app: &tauri::AppHandle, tool_slug: &str) -> Result<Vec<String>, String> {
    let wasm_name = format!("{tool_slug}.wasm");
    let capabilities_name = format!("{tool_slug}.capabilities.json");
    let mut found_paths: Option<(PathBuf, PathBuf)> = None;
    let mut attempted_paths: Vec<(PathBuf, PathBuf)> = Vec::new();
    for resource_dir in bundled_tools_resource_dirs(app) {
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
        format!("未找到内置工具文件，请先执行构建工具资源。已尝试: {attempted}")
    })?;
    let target_dir = ironclaw_tools_dir(app)?;
    fs::create_dir_all(&target_dir).map_err(|e| format!("创建工具目录失败: {e}"))?;
    let target_wasm = target_dir.join(&wasm_name);
    let target_capabilities = target_dir.join(&capabilities_name);
    fs::copy(&source_wasm, &target_wasm).map_err(|e| format!("复制工具 wasm 失败: {e}"))?;
    fs::copy(&source_capabilities, &target_capabilities)
        .map_err(|e| format!("复制工具 capabilities 失败: {e}"))?;
    Ok(vec![
        target_wasm.display().to_string(),
        target_capabilities.display().to_string(),
    ])
}

fn list_bundled_tools(app: &tauri::AppHandle) -> Result<Vec<String>, String> {
    let mut names = std::collections::BTreeSet::new();
    for resource_dir in bundled_tools_resource_dirs(app) {
        if !resource_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&resource_dir).map_err(|e| format!("遍历工具资源失败: {e}"))? {
            let entry = entry.map_err(|e| format!("读取工具资源失败: {e}"))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !file_name.ends_with(".wasm") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let capabilities_path = resource_dir.join(format!("{stem}.capabilities.json"));
            if capabilities_path.is_file() {
                names.insert(stem.to_string());
            }
        }
    }
    Ok(names.into_iter().collect())
}

fn is_http_zip_source(source: &str) -> bool {
    let normalized = source.trim().to_ascii_lowercase();
    (normalized.starts_with("http://") || normalized.starts_with("https://"))
        && normalized.ends_with(".zip")
}

fn normalize_tool_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("遍历目录失败: {e}"))? {
        let entry = entry.map_err(|e| format!("读取目录项失败: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn copy_dir_recursive(source_dir: &Path, target_dir: &Path) -> Result<Vec<String>, String> {
    if !source_dir.is_dir() {
        return Err(format!("源目录不存在: {}", source_dir.display()));
    }
    fs::create_dir_all(target_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let mut copied = Vec::new();
    for entry in fs::read_dir(source_dir).map_err(|e| format!("遍历目录失败: {e}"))? {
        let entry = entry.map_err(|e| format!("读取目录项失败: {e}"))?;
        let source_path = entry.path();
        let target_path = target_dir.join(entry.file_name());
        if source_path.is_dir() {
            let nested = copy_dir_recursive(&source_path, &target_path)?;
            copied.extend(nested);
        } else if source_path.is_file() {
            fs::copy(&source_path, &target_path).map_err(|e| format!("复制文件失败: {e}"))?;
            copied.push(target_path.display().to_string());
        }
    }
    Ok(copied)
}

fn extract_zip_bytes_to_dir(bytes: &[u8], output_dir: &Path) -> Result<(), String> {
    let reader = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("读取 zip 压缩包失败: {e}"))?;
    fs::create_dir_all(output_dir).map_err(|e| format!("创建解压目录失败: {e}"))?;
    for index in 0..archive.len() {
        let mut zipped = archive
            .by_index(index)
            .map_err(|e| format!("读取压缩包条目失败: {e}"))?;
        let Some(enclosed) = zipped.enclosed_name().map(PathBuf::from) else {
            continue;
        };
        let out_path = output_dir.join(enclosed);
        if zipped.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("创建目录失败: {e}"))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
            }
            let mut out_file =
                fs::File::create(&out_path).map_err(|e| format!("创建文件失败: {e}"))?;
            std::io::copy(&mut zipped, &mut out_file).map_err(|e| format!("写入文件失败: {e}"))?;
        }
    }
    Ok(())
}

fn install_tool_from_zip_url(
    app: &tauri::AppHandle,
    tool_slug: &str,
    zip_url: &str,
) -> Result<Vec<String>, String> {
    let response = reqwest::blocking::get(zip_url)
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("下载工具 zip 失败: {e}"))?;
    let bytes = response
        .bytes()
        .map_err(|e| format!("读取工具 zip 内容失败: {e}"))?;

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let temp_root = env::temp_dir().join(format!(
        "ironclaw-tool-install-{}-{millis}",
        std::process::id()
    ));
    let extracted_dir = temp_root.join("extract");
    extract_zip_bytes_to_dir(bytes.as_ref(), &extracted_dir)?;

    let mut extracted_files = Vec::new();
    collect_files_recursive(&extracted_dir, &mut extracted_files)?;
    let normalized_slug = normalize_tool_name(tool_slug);

    let mut wasm_candidates = extracted_files
        .iter()
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("wasm"))
        .cloned()
        .collect::<Vec<_>>();
    wasm_candidates.sort();
    let selected_wasm = wasm_candidates
        .iter()
        .find(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| normalize_tool_name(s) == normalized_slug)
                .unwrap_or(false)
        })
        .cloned()
        .or_else(|| wasm_candidates.first().cloned())
        .ok_or_else(|| "zip 中未找到 wasm 工具文件".to_string())?;

    let mut capabilities_candidates = extracted_files
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|s| s.to_str())
                .map(|name| name.ends_with(".capabilities.json"))
                .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    capabilities_candidates.sort();

    let selected_capabilities = capabilities_candidates
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(|s| s.to_str())
                .map(|name| {
                    let base = name.trim_end_matches(".capabilities.json");
                    normalize_tool_name(base) == normalized_slug
                })
                .unwrap_or(false)
        })
        .cloned()
        .or_else(|| capabilities_candidates.first().cloned())
        .ok_or_else(|| "zip 中未找到 capabilities 文件".to_string())?;

    let target_dir = ironclaw_tools_dir(app)?;
    fs::create_dir_all(&target_dir).map_err(|e| format!("创建工具目录失败: {e}"))?;
    let target_wasm = target_dir.join(format!("{tool_slug}.wasm"));
    let target_capabilities = target_dir.join(format!("{tool_slug}.capabilities.json"));
    fs::copy(&selected_wasm, &target_wasm).map_err(|e| format!("复制工具 wasm 失败: {e}"))?;
    fs::copy(&selected_capabilities, &target_capabilities)
        .map_err(|e| format!("复制工具 capabilities 失败: {e}"))?;
    let _ = fs::remove_dir_all(&temp_root);
    Ok(vec![
        target_wasm.display().to_string(),
        target_capabilities.display().to_string(),
    ])
}

fn is_http_source(source: &str) -> bool {
    let normalized = source.trim().to_ascii_lowercase();
    normalized.starts_with("http://") || normalized.starts_with("https://")
}

fn sanitize_skill_slug(value: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if prev_dash {
                continue;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
        out.push(mapped);
    }
    out.trim_matches('-').to_string()
}

fn extract_skill_name_from_markdown(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("name:") {
            let raw = value.trim().trim_matches('"').trim_matches('\'');
            let slug = sanitize_skill_slug(raw);
            if !slug.is_empty() {
                return Some(slug);
            }
        }
    }
    None
}

fn derive_skill_slug_from_url(source_url: &str) -> Option<String> {
    let mut parts = source_url
        .trim()
        .split('?')
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let last = parts.pop()?;
    let base = last
        .trim_end_matches(".md")
        .trim_end_matches(".markdown")
        .trim_end_matches(".zip")
        .trim_end_matches(".txt");
    let slug = sanitize_skill_slug(base);
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

fn install_skill_from_markdown_url(
    app: &tauri::AppHandle,
    source_url: &str,
) -> Result<(String, Vec<String>), String> {
    let content = reqwest::blocking::get(source_url)
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("下载技能失败: {e}"))?
        .text()
        .map_err(|e| format!("读取技能内容失败: {e}"))?;
    let skill_slug = extract_skill_name_from_markdown(&content)
        .or_else(|| derive_skill_slug_from_url(source_url))
        .ok_or_else(|| "无法从技能内容或 URL 推断技能名称".to_string())?;
    let target_dir = ironclaw_installed_skills_dir(app)?.join(&skill_slug);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).map_err(|e| format!("清理旧技能目录失败: {e}"))?;
    }
    fs::create_dir_all(&target_dir).map_err(|e| format!("创建技能目录失败: {e}"))?;
    let skill_md = target_dir.join("SKILL.md");
    fs::write(&skill_md, content).map_err(|e| format!("写入技能文件失败: {e}"))?;
    Ok((skill_slug, vec![skill_md.display().to_string()]))
}

fn install_skill_from_zip_url(
    app: &tauri::AppHandle,
    zip_url: &str,
) -> Result<(String, Vec<String>), String> {
    let response = reqwest::blocking::get(zip_url)
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("下载技能 zip 失败: {e}"))?;
    let bytes = response
        .bytes()
        .map_err(|e| format!("读取技能 zip 内容失败: {e}"))?;

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let temp_root = env::temp_dir().join(format!(
        "ironclaw-skill-install-{}-{millis}",
        std::process::id()
    ));
    let extracted_dir = temp_root.join("extract");
    extract_zip_bytes_to_dir(bytes.as_ref(), &extracted_dir)?;

    let mut extracted_files = Vec::new();
    collect_files_recursive(&extracted_dir, &mut extracted_files)?;
    let mut skill_md_candidates = extracted_files
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|s| s.to_str())
                .map(|name| name == "SKILL.md")
                .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    skill_md_candidates.sort();

    let selected_skill_md = skill_md_candidates
        .first()
        .cloned()
        .ok_or_else(|| "zip 中未找到 SKILL.md".to_string())?;

    let skill_content = fs::read_to_string(&selected_skill_md).unwrap_or_default();
    let skill_slug = extract_skill_name_from_markdown(&skill_content)
        .or_else(|| {
            selected_skill_md
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .map(sanitize_skill_slug)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| derive_skill_slug_from_url(zip_url))
        .ok_or_else(|| "无法从 zip 内容或 URL 推断技能名称".to_string())?;

    let source_skill_dir = selected_skill_md
        .parent()
        .ok_or_else(|| "技能目录解析失败".to_string())?
        .to_path_buf();
    let target_dir = ironclaw_installed_skills_dir(app)?.join(&skill_slug);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).map_err(|e| format!("清理旧技能目录失败: {e}"))?;
    }
    let copied_files = copy_dir_recursive(&source_skill_dir, &target_dir)?;
    let _ = fs::remove_dir_all(&temp_root);
    Ok((skill_slug, copied_files))
}

fn ensure_bundled_skills_installed(app: &tauri::AppHandle) -> Result<usize, String> {
    let target_root = ironclaw_skills_dir(app)?;
    fs::create_dir_all(&target_root).map_err(|e| format!("创建技能目录失败: {e}"))?;
    let mut installed_count = 0usize;
    for resource_dir in bundled_skills_resource_dirs(app) {
        if !resource_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&resource_dir).map_err(|e| format!("遍历技能资源失败: {e}"))? {
            let entry = entry.map_err(|e| format!("读取技能资源失败: {e}"))?;
            let source_path = entry.path();
            if !source_path.is_dir() || !source_path.join("SKILL.md").is_file() {
                continue;
            }
            let skill_name = entry.file_name();
            let target_path = target_root.join(&skill_name);
            if target_path.join("SKILL.md").is_file() {
                continue;
            }
            let _ = copy_dir_recursive(&source_path, &target_path)?;
            installed_count += 1;
        }
    }
    Ok(installed_count)
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
fn save_tool_config(app: tauri::AppHandle, payload: ToolSavePayload) -> Result<ToolSaveResult, String> {
    let tool_slug = normalize_tool_name(payload.tool_name.trim());
    if tool_slug.is_empty() {
        return Err("工具名称不能为空".to_string());
    }

    let source = normalize_optional(payload.install_source.clone());
    let (installed_files, message) = if let Some(value) = source {
        if is_http_zip_source(&value) {
            (
                install_tool_from_zip_url(&app, &tool_slug, &value)?,
                format!("工具已通过网络安装到 ~/.ironclaw/tools: {tool_slug}"),
            )
        } else {
            (
                install_bundled_tool(&app, &tool_slug)?,
                format!("工具已安装到 ~/.ironclaw/tools: {tool_slug}"),
            )
        }
    } else {
        (
            install_bundled_tool(&app, &tool_slug)?,
            format!("工具已安装到 ~/.ironclaw/tools: {tool_slug}"),
        )
    };

    Ok(ToolSaveResult {
        tool_name: tool_slug,
        installed_files,
        message,
    })
}

#[tauri::command]
fn get_bundled_tools(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    list_bundled_tools(&app)
}

#[tauri::command]
fn save_skill_config(app: tauri::AppHandle, payload: SkillSavePayload) -> Result<SkillSaveResult, String> {
    let source = payload.install_source.trim();
    if !is_http_source(source) {
        return Err("技能安装仅支持 URL".to_string());
    }
    let (skill_slug, installed_files) = if is_http_zip_source(source) {
        install_skill_from_zip_url(&app, source)?
    } else {
        install_skill_from_markdown_url(&app, source)?
    };
    let message = format!("技能已通过网络安装到 ~/.ironclaw/installed_skills: {skill_slug}");

    Ok(SkillSaveResult {
        skill_name: skill_slug,
        installed_files,
        message,
    })
}

#[tauri::command]
fn save_tunnel_config(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    payload: TunnelSavePayload,
) -> Result<TunnelSaveResult, String> {
    let username = payload.username.trim();
    if username.is_empty() {
        return Err("用户名不能为空".to_string());
    }
    let password = payload.password.trim();
    if password.is_empty() {
        return Err("密码不能为空".to_string());
    }
    let node_id = match payload.node_id.as_deref().map(str::trim) {
        Some(value) if value.len() >= 32 => value.to_string(),
        _ => generate_tunnel_node_id(),
    };
    let verification_token = payload.verification_token.trim();
    if verification_token.is_empty() {
        return Err("Verification Token 不能为空".to_string());
    }
    let channel_slug = tunnel_channel_slug(&payload.channel_type).to_string();
    let (binary_path, config_path) = ensure_tunnel_runtime_assets(&app)?;
    let yaml_raw = fs::read_to_string(&config_path).map_err(|e| format!("读取 workbot.yaml 失败: {e}"))?;
    let yaml_with_node_id = update_workbot_yaml_field_in_section(&yaml_raw, "client", "node_id", &node_id);
    let yaml_with_user = update_workbot_yaml_field_in_section(&yaml_with_node_id, "auth", "username", username);
    let yaml_updated = update_workbot_yaml_field_in_section(&yaml_with_user, "auth", "password", password);
    fs::write(&config_path, yaml_updated).map_err(|e| format!("写入 workbot.yaml 失败: {e}"))?;
    let binary_name = binary_path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| "读取 tunnel 客户端名称失败".to_string())?
        .to_string();
    let command = if cfg!(target_os = "windows") {
        format!("{binary_name} -config .\\workbot.yaml")
    } else {
        format!("{binary_name} -config ./workbot.yaml")
    };
    let workdir_path = binary_path
        .parent()
        .ok_or_else(|| "读取 tunnel 工作目录失败".to_string())?
        .to_path_buf();
    let tunnel_pid = {
        let mut tunnel_guard = state
            .tunnel_child
            .lock()
            .map_err(|_| "无法获取 tunnel 进程状态锁".to_string())?;
        if let Some(existing) = tunnel_guard.as_mut() {
            match existing.try_wait() {
                Ok(Some(_)) => {
                    *tunnel_guard = None;
                }
                Ok(None) => {
                    let old_pid = existing.id();
                    existing
                        .kill()
                        .map_err(|e| format!("停止旧 tunnel 进程失败 (pid={old_pid}): {e}"))?;
                    let _ = existing.wait();
                    *tunnel_guard = None;
                }
                Err(e) => {
                    return Err(format!("检查旧 tunnel 进程状态失败: {e}"));
                }
            }
        }
        let mut tunnel_cmd = Command::new(&binary_path);
        tunnel_cmd
            .arg("-config")
            .arg("./workbot.yaml")
            .current_dir(&workdir_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let tunnel_child = tunnel_cmd
            .spawn()
            .map_err(|e| format!("启动 tunnel 客户端失败: {e}"))?;
        let pid = tunnel_child.id();
        *tunnel_guard = Some(tunnel_child);
        pid
    };
    let tunnel_url = format!(
        "https://workbot.axiayun.com/proxy/{username}/{node_id}/webhook/{channel_slug}?secret={verification_token}"
    );
    Ok(TunnelSaveResult {
        username: username.to_string(),
        node_id,
        channel_slug,
        command,
        workdir: workdir_path.display().to_string(),
        config_path: config_path.display().to_string(),
        binary_path: binary_path.display().to_string(),
        tunnel_url,
        tunnel_pid,
        message: format!("通道配置已保存并重启 tunnel 进程 (pid={tunnel_pid})"),
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

    let bundled_skills_synced = ensure_bundled_skills_installed(&app)?;

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
        message: if bundled_skills_synced > 0 {
            format!("已启动 ironclaw run (pid={pid})，已同步 {bundled_skills_synced} 个内置技能")
        } else {
            format!("已启动 ironclaw run (pid={pid})")
        },
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
    let parsed_url: tauri::Url = url
        .parse()
        .map_err(|e| format!("控制台地址无效: {e}"))?;
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err("控制台地址仅支持 http/https".to_string());
    }
    app.opener()
        .open_url(parsed_url.as_str(), None::<&str>)
        .map_err(|e| format!("打开系统浏览器失败: {e}"))?;
    Ok(())
}

#[tauri::command]
fn open_console_popup_in_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed_url: tauri::Url = url
        .parse()
        .map_err(|e| format!("弹窗地址无效: {e}"))?;
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err("弹窗地址仅支持 http/https".to_string());
    }
    app.opener()
        .open_url(parsed_url.as_str(), None::<&str>)
        .map_err(|e| format!("打开系统浏览器失败: {e}"))?;
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
            save_tool_config,
            get_bundled_tools,
            save_skill_config,
            save_tunnel_config,
            get_ironclaw_logs,
            start_ironclaw_run,
            stop_ironclaw_run,
            get_ironclaw_run_status,
            open_console_window,
            open_console_popup_in_browser
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
