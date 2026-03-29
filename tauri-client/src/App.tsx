import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

type MainTab = "workspace" | "employees";
type WorkspaceSubtab = "models" | "channels" | "tools" | "skills" | "logs";
type UiMode = "light" | "dark";

type IronclawRunStatus = {
  running: boolean;
  pid: number | null;
  message: string;
};

type GatewayInfo = {
  url: string;
  hasToken: boolean;
  token: string | null;
};

type IronclawLogs = {
  content: string;
};

type LaunchEnvConfig = {
  llmBackend: string | null;
  llmBaseUrl: string | null;
  llmModel: string | null;
  llmApiKey: string | null;
  openaiApiKey: string | null;
  anthropicApiKey: string | null;
  nearaiApiKey: string | null;
  ollamaBaseUrl: string | null;
  feishuAppId: string | null;
  feishuAppSecret: string | null;
  feishuVerificationToken: string | null;
  telegramBotToken: string | null;
  telegramWebhookSecret: string | null;
  slackBotToken: string | null;
  slackSigningSecret: string | null;
  discordBotToken: string | null;
  discordPublicKey: string | null;
  whatsappAccessToken: string | null;
  whatsappVerifyToken: string | null;
};

type ChannelSaveResult = {
  channelType: string;
  installedFiles: string[];
  message: string;
};

type ToolSaveResult = {
  toolName: string;
  installedFiles: string[];
  message: string;
};

type SkillSaveResult = {
  skillName: string;
  installedFiles: string[];
  message: string;
};

type TunnelSaveResult = {
  username: string;
  nodeId: string;
  channelSlug: string;
  command: string;
  workdir: string;
  configPath: string;
  binaryPath: string;
  tunnelUrl: string;
  tunnelPid: number;
  message: string;
};

type TauriInternals = {
  invoke?: (cmd: string, args?: Record<string, unknown>, options?: unknown) => Promise<unknown>;
};

type WorkspaceConfig = {
  gatewayToken: string;
  modelProtocol: string;
  modelProviderId: string;
  modelBaseUrl: string;
  modelApiKey: string;
  modelId: string;
  channelType: string;
  channelAppIdOrToken: string;
  channelAppSecret: string;
  channelVerificationToken: string;
  tunnelUsername: string;
  tunnelPassword: string;
  tunnelNodeId: string;
  tunnelStartCommand: string;
  tunnelWorkdir: string;
  tunnelAddress: string;
  toolName: string;
  toolInstallSource: string;
  skillName: string;
  skillInstallSource: string;
};

const WORKSPACE_CONFIG_STORAGE_KEY = "ironclaw.workspace.config.v1";
const UI_MODE_STORAGE_KEY = "ironclaw.ui.mode.v1";

const EMPTY_CONFIG: WorkspaceConfig = {
  gatewayToken: "",
  modelProtocol: "openai-completions",
  modelProviderId: "",
  modelBaseUrl: "",
  modelApiKey: "",
  modelId: "",
  channelType: "Feishu",
  channelAppIdOrToken: "",
  channelAppSecret: "",
  channelVerificationToken: "",
  tunnelUsername: "xm",
  tunnelPassword: "123123",
  tunnelNodeId: "",
  tunnelStartCommand: "",
  tunnelWorkdir: "",
  tunnelAddress: "",
  toolName: "",
  toolInstallSource: "",
  skillName: "",
  skillInstallSource: "",
};

function getTauriInternals(): TauriInternals | undefined {
  return (window as typeof window & { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
}

async function waitForTauriBridge(timeoutMs = 8000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (getTauriInternals()?.invoke) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Tauri IPC 未注入: origin=${location.origin}`);
}

async function invokeTauri<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  await waitForTauriBridge();
  return invoke<T>(cmd, args);
}

function readStoredConfig() {
  const raw = localStorage.getItem(WORKSPACE_CONFIG_STORAGE_KEY);
  if (!raw) {
    return EMPTY_CONFIG;
  }
  try {
    const parsed = JSON.parse(raw) as Partial<WorkspaceConfig>;
    return { ...EMPTY_CONFIG, ...parsed };
  } catch {
    return EMPTY_CONFIG;
  }
}

function readStoredUiMode(): UiMode {
  const raw = localStorage.getItem(UI_MODE_STORAGE_KEY);
  if (raw === "light") {
    return "light";
  }
  return "dark";
}

function protocolToBackend(protocol: string) {
  if (protocol === "anthropic-messages") {
    return "anthropic";
  }
  return "openai_compatible";
}

function backendToProtocol(backend: string | null | undefined) {
  if (backend === "anthropic") {
    return "anthropic-messages";
  }
  return "openai-completions";
}

function channelTypeToSlug(channelType: string) {
  if (channelType === "Telegram") {
    return "telegram";
  }
  if (channelType === "Slack") {
    return "slack";
  }
  if (channelType === "Discord") {
    return "discord";
  }
  if (channelType === "WhatsApp") {
    return "whatsapp";
  }
  return "feishu";
}

function generateNodeId() {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID().replace(/-/g, "");
  }
  const text = `${Date.now().toString(16)}${Math.random().toString(16).slice(2)}${Math.random().toString(16).slice(2)}`;
  return text.padEnd(32, "0").slice(0, 32);
}

function resolveChannelFormFromLaunchConfig(launchConfig: LaunchEnvConfig, channelType: string) {
  if (channelType === "Telegram") {
    return {
      appIdOrToken: launchConfig.telegramBotToken ?? "",
      appSecret: "",
      verificationToken: launchConfig.telegramWebhookSecret ?? "",
    };
  }
  if (channelType === "Slack") {
    return {
      appIdOrToken: launchConfig.slackBotToken ?? "",
      appSecret: launchConfig.slackSigningSecret ?? "",
      verificationToken: "",
    };
  }
  if (channelType === "Discord") {
    return {
      appIdOrToken: launchConfig.discordBotToken ?? "",
      appSecret: launchConfig.discordPublicKey ?? "",
      verificationToken: "",
    };
  }
  if (channelType === "WhatsApp") {
    return {
      appIdOrToken: launchConfig.whatsappAccessToken ?? "",
      appSecret: "",
      verificationToken: launchConfig.whatsappVerifyToken ?? "",
    };
  }
  return {
    appIdOrToken: launchConfig.feishuAppId ?? "",
    appSecret: launchConfig.feishuAppSecret ?? "",
    verificationToken: launchConfig.feishuVerificationToken ?? "",
  };
}

function getChannelFieldMeta(channelType: string) {
  if (channelType === "Telegram") {
    return {
      firstLabel: "Bot Token",
      firstPlaceholder: "输入 TELEGRAM_BOT_TOKEN",
      secondLabel: "预留字段",
      secondPlaceholder: "Telegram 暂无第二必填项",
      thirdLabel: "Webhook Secret",
      thirdPlaceholder: "可选：TELEGRAM_WEBHOOK_SECRET",
      secondDisabled: true,
      thirdDisabled: false,
    };
  }
  if (channelType === "Slack") {
    return {
      firstLabel: "Bot Token",
      firstPlaceholder: "输入 SLACK_BOT_TOKEN",
      secondLabel: "Signing Secret",
      secondPlaceholder: "输入 SLACK_SIGNING_SECRET",
      thirdLabel: "预留字段",
      thirdPlaceholder: "Slack 暂无第三必填项",
      secondDisabled: false,
      thirdDisabled: true,
    };
  }
  if (channelType === "Discord") {
    return {
      firstLabel: "Bot Token",
      firstPlaceholder: "输入 DISCORD_BOT_TOKEN",
      secondLabel: "Public Key",
      secondPlaceholder: "输入 DISCORD_PUBLIC_KEY",
      thirdLabel: "预留字段",
      thirdPlaceholder: "Discord 暂无第三必填项",
      secondDisabled: false,
      thirdDisabled: true,
    };
  }
  if (channelType === "WhatsApp") {
    return {
      firstLabel: "Access Token",
      firstPlaceholder: "输入 WHATSAPP_ACCESS_TOKEN",
      secondLabel: "预留字段",
      secondPlaceholder: "WhatsApp 暂无第二必填项",
      thirdLabel: "Verify Token",
      thirdPlaceholder: "输入 WHATSAPP_VERIFY_TOKEN",
      secondDisabled: true,
      thirdDisabled: false,
    };
  }
  return {
    firstLabel: "App ID",
    firstPlaceholder: "输入 FEISHU_APP_ID",
    secondLabel: "App Secret",
    secondPlaceholder: "输入 FEISHU_APP_SECRET",
    thirdLabel: "Verification Token",
    thirdPlaceholder: "输入 FEISHU_VERIFICATION_TOKEN",
    secondDisabled: false,
    thirdDisabled: false,
  };
}

function App() {
  const [activeTab, setActiveTab] = useState<MainTab>("workspace");
  const [activeSubtab, setActiveSubtab] = useState<WorkspaceSubtab>("models");
  const [runtimeStatus, setRuntimeStatus] = useState<IronclawRunStatus>({
    running: false,
    pid: null,
    message: "",
  });
  const [runtimeResult, setRuntimeResult] = useState("已内置 ironclaw 运行时");
  const [gatewayUrl, setGatewayUrl] = useState("http://127.0.0.1:3000/");
  const [workspaceConfig, setWorkspaceConfig] = useState<WorkspaceConfig>(readStoredConfig);
  const [uiMode, setUiMode] = useState<UiMode>(readStoredUiMode);
  const [bundledTools, setBundledTools] = useState<string[]>([]);
  const [logsContent, setLogsContent] = useState("");
  const isDark = uiMode === "dark";

  useEffect(() => {
    localStorage.setItem(WORKSPACE_CONFIG_STORAGE_KEY, JSON.stringify(workspaceConfig));
  }, [workspaceConfig]);

  useEffect(() => {
    localStorage.setItem(UI_MODE_STORAGE_KEY, uiMode);
  }, [uiMode]);

  const updateConfig = (key: keyof WorkspaceConfig, value: string) => {
    setWorkspaceConfig((current) => ({ ...current, [key]: value }));
  };

  const loadLaunchEnvConfig = async () => {
    try {
      const launchConfig = await invokeTauri<LaunchEnvConfig>("get_launch_env_config");
      setWorkspaceConfig((current) => ({
        ...current,
        modelProtocol: backendToProtocol(launchConfig.llmBackend),
        modelBaseUrl: launchConfig.llmBaseUrl ?? current.modelBaseUrl,
        modelApiKey: launchConfig.llmApiKey ?? current.modelApiKey,
        modelId: launchConfig.llmModel ?? current.modelId,
        channelAppIdOrToken: resolveChannelFormFromLaunchConfig(launchConfig, current.channelType).appIdOrToken,
        channelAppSecret: resolveChannelFormFromLaunchConfig(launchConfig, current.channelType).appSecret,
        channelVerificationToken: resolveChannelFormFromLaunchConfig(launchConfig, current.channelType).verificationToken,
      }));
    } catch {
      setRuntimeResult("读取已保存模型环境变量失败");
    }
  };

  const saveLaunchEnvConfig = async (showMessage: boolean) => {
    const existing = await invokeTauri<LaunchEnvConfig>("get_launch_env_config");
    const payload: LaunchEnvConfig = {
      ...existing,
      llmBackend: protocolToBackend(workspaceConfig.modelProtocol),
      llmBaseUrl: workspaceConfig.modelBaseUrl || null,
      llmModel: workspaceConfig.modelId || null,
      llmApiKey: workspaceConfig.modelApiKey || null,
      feishuAppId: workspaceConfig.channelType === "Feishu" ? workspaceConfig.channelAppIdOrToken || null : existing.feishuAppId,
      feishuAppSecret: workspaceConfig.channelType === "Feishu" ? workspaceConfig.channelAppSecret || null : existing.feishuAppSecret,
      feishuVerificationToken:
        workspaceConfig.channelType === "Feishu" ? workspaceConfig.channelVerificationToken || null : existing.feishuVerificationToken,
      telegramBotToken: workspaceConfig.channelType === "Telegram" ? workspaceConfig.channelAppIdOrToken || null : existing.telegramBotToken,
      telegramWebhookSecret:
        workspaceConfig.channelType === "Telegram" ? workspaceConfig.channelVerificationToken || null : existing.telegramWebhookSecret,
      slackBotToken: workspaceConfig.channelType === "Slack" ? workspaceConfig.channelAppIdOrToken || null : existing.slackBotToken,
      slackSigningSecret: workspaceConfig.channelType === "Slack" ? workspaceConfig.channelAppSecret || null : existing.slackSigningSecret,
      discordBotToken: workspaceConfig.channelType === "Discord" ? workspaceConfig.channelAppIdOrToken || null : existing.discordBotToken,
      discordPublicKey: workspaceConfig.channelType === "Discord" ? workspaceConfig.channelAppSecret || null : existing.discordPublicKey,
      whatsappAccessToken:
        workspaceConfig.channelType === "WhatsApp" ? workspaceConfig.channelAppIdOrToken || null : existing.whatsappAccessToken,
      whatsappVerifyToken:
        workspaceConfig.channelType === "WhatsApp" ? workspaceConfig.channelVerificationToken || null : existing.whatsappVerifyToken,
    };
    const saved = await invokeTauri<LaunchEnvConfig>("set_launch_env_config", { config: payload });
    setWorkspaceConfig((current) => ({
      ...current,
      modelProtocol: backendToProtocol(saved.llmBackend),
      modelBaseUrl: saved.llmBaseUrl ?? "",
      modelApiKey: saved.llmApiKey ?? "",
      modelId: saved.llmModel ?? "",
      channelAppIdOrToken: resolveChannelFormFromLaunchConfig(saved, current.channelType).appIdOrToken,
      channelAppSecret: resolveChannelFormFromLaunchConfig(saved, current.channelType).appSecret,
      channelVerificationToken: resolveChannelFormFromLaunchConfig(saved, current.channelType).verificationToken,
    }));
    if (showMessage) {
      setRuntimeResult("模型参数已写入环境变量配置，后续启动将自动注入");
    }
  };

  const refreshGatewayInfo = async () => {
    try {
      const info = await invokeTauri<GatewayInfo>("get_gateway_info");
      setGatewayUrl(info.url);
      setWorkspaceConfig((current) => ({ ...current, gatewayToken: info.token ?? "" }));
    } catch {
      setGatewayUrl("http://127.0.0.1:3000/");
    }
  };

  const refreshIronclawRunStatus = async () => {
    try {
      const status = await invokeTauri<IronclawRunStatus>("get_ironclaw_run_status");
      setRuntimeStatus(status);
    } catch {
      setRuntimeStatus({ running: false, pid: null, message: "状态读取失败" });
    }
  };

  const applyGatewayToken = async () => {
    setRuntimeResult("正在应用 GATEWAY_AUTH_TOKEN 并重启服务...");
    try {
      const info = await invokeTauri<GatewayInfo>("set_gateway_auth_token", {
        token: workspaceConfig.gatewayToken,
      });
      setGatewayUrl(info.url);
      setWorkspaceConfig((current) => ({ ...current, gatewayToken: info.token ?? "" }));
      const stopStatus = await invokeTauri<IronclawRunStatus>("stop_ironclaw_run");
      setRuntimeStatus(stopStatus);
      const startStatus = await invokeTauri<IronclawRunStatus>("start_ironclaw_run");
      setRuntimeStatus(startStatus);
      setRuntimeResult(info.hasToken ? "Token 已应用并重启服务" : "已清空自定义 Token，并重启服务");
      await refreshGatewayInfo();
      await fetchIronclawLogs();
    } catch (error) {
      setRuntimeResult(`应用 Token 失败: ${String(error)}`);
    }
  };

  const startIronclawRun = async () => {
    setRuntimeResult("正在写入模型环境变量并启动 ironclaw run...");
    try {
      await saveLaunchEnvConfig(false);
      const status = await invokeTauri<IronclawRunStatus>("start_ironclaw_run");
      setRuntimeStatus(status);
      setRuntimeResult(status.message);
      await refreshGatewayInfo();
    } catch (error) {
      setRuntimeResult(`启动失败: ${String(error)}`);
    }
  };

  const stopIronclawRun = async () => {
    setRuntimeResult("正在停止 ironclaw run...");
    try {
      const status = await invokeTauri<IronclawRunStatus>("stop_ironclaw_run");
      setRuntimeStatus(status);
      setRuntimeResult(status.message);
    } catch (error) {
      setRuntimeResult(`停止失败: ${String(error)}`);
    }
  };

  const fetchIronclawLogs = async () => {
    try {
      const logs = await invokeTauri<IronclawLogs>("get_ironclaw_logs");
      setLogsContent(logs.content);
    } catch {
      setLogsContent("日志读取失败");
    }
  };

  const saveChannelConfig = async () => {
    setRuntimeResult("正在保存通道配置并安装内置通道...");
    try {
      const result = await invokeTauri<ChannelSaveResult>("save_channel_config", {
        payload: {
          channelType: workspaceConfig.channelType,
          appIdOrToken: workspaceConfig.channelAppIdOrToken || null,
          appSecret: workspaceConfig.channelAppSecret || null,
          verificationToken: workspaceConfig.channelVerificationToken || null,
        },
      });
      setRuntimeResult(`${result.message}，已写入 ${result.installedFiles.length} 个文件`);
      await loadLaunchEnvConfig();
    } catch (error) {
      setRuntimeResult(`保存通道失败: ${String(error)}`);
    }
  };

  const saveTunnelConfig = async () => {
    const username = workspaceConfig.tunnelUsername.trim();
    const password = workspaceConfig.tunnelPassword.trim();
    const verificationToken = workspaceConfig.channelVerificationToken.trim();
    if (!username) {
      setRuntimeResult("请先填写通道用户名");
      return;
    }
    if (!password) {
      setRuntimeResult("请先填写通道密码");
      return;
    }
    if (!verificationToken) {
      setRuntimeResult("请先填写通道 Verification Token");
      return;
    }
    const nodeId = workspaceConfig.tunnelNodeId.trim().length >= 32 ? workspaceConfig.tunnelNodeId.trim() : generateNodeId();
    setRuntimeResult("正在保存 workbot.yaml 并重启 tunnel 进程...");
    try {
      const result = await invokeTauri<TunnelSaveResult>("save_tunnel_config", {
        payload: {
          username,
          password,
          nodeId,
          channelType: workspaceConfig.channelType,
          verificationToken,
        },
      });
      setWorkspaceConfig((current) => ({
        ...current,
        tunnelUsername: result.username,
        tunnelNodeId: result.nodeId,
        tunnelStartCommand: result.command,
        tunnelWorkdir: result.workdir,
        tunnelAddress: result.tunnelUrl,
      }));
      setRuntimeResult(`${result.message}，启动命令：cd ${result.workdir} && ${result.command}`);
    } catch (error) {
      setRuntimeResult(`保存通道启动配置失败: ${String(error)}`);
    }
  };

  const saveToolConfig = async () => {
    if (!workspaceConfig.toolName.trim()) {
      setRuntimeResult("请选择或填写工具名称");
      return;
    }
    setRuntimeResult("正在安装工具...");
    try {
      const result = await invokeTauri<ToolSaveResult>("save_tool_config", {
        payload: {
          toolName: workspaceConfig.toolName,
          installSource: workspaceConfig.toolInstallSource || null,
        },
      });
      setRuntimeResult(`${result.message}，已写入 ${result.installedFiles.length} 个文件`);
    } catch (error) {
      setRuntimeResult(`安装工具失败: ${String(error)}`);
    }
  };

  const loadBundledTools = async () => {
    try {
      const tools = await invokeTauri<string[]>("get_bundled_tools");
      setBundledTools(tools);
    } catch {
      setBundledTools([]);
    }
  };

  const saveSkillConfig = async () => {
    if (!workspaceConfig.skillName.trim()) {
      setRuntimeResult("请输入技能名称");
      return;
    }
    if (!workspaceConfig.skillInstallSource.trim()) {
      setRuntimeResult("请输入技能 URL");
      return;
    }
    setRuntimeResult("正在安装技能...");
    try {
      const result = await invokeTauri<SkillSaveResult>("save_skill_config", {
        payload: {
          skillName: workspaceConfig.skillName,
          installSource: workspaceConfig.skillInstallSource,
        },
      });
      setRuntimeResult(`${result.message}，已写入 ${result.installedFiles.length} 个文件`);
    } catch (error) {
      setRuntimeResult(`安装技能失败: ${String(error)}`);
    }
  };

  const saveModelAndRestart = async () => {
    setRuntimeResult("正在保存模型配置并重启 ironclaw...");
    try {
      await saveLaunchEnvConfig(false);
      const stopStatus = await invokeTauri<IronclawRunStatus>("stop_ironclaw_run");
      setRuntimeStatus(stopStatus);
      const startStatus = await invokeTauri<IronclawRunStatus>("start_ironclaw_run");
      setRuntimeStatus(startStatus);
      setRuntimeResult("模型配置已保存并重启 ironclaw");
      await refreshGatewayInfo();
      await fetchIronclawLogs();
    } catch (error) {
      setRuntimeResult(`保存并重启失败: ${String(error)}`);
    }
  };

  const openConsoleLink = async () => {
    await openUrl(gatewayUrl);
  };

  useEffect(() => {
    void refreshGatewayInfo();
    void refreshIronclawRunStatus();
    void loadLaunchEnvConfig();
    void loadBundledTools();
    void fetchIronclawLogs();
  }, []);

  useEffect(() => {
    if (!bundledTools.length) {
      return;
    }
    if (!workspaceConfig.toolInstallSource.trim() && !workspaceConfig.toolName.trim()) {
      setWorkspaceConfig((current) => ({ ...current, toolName: bundledTools[0] }));
    }
  }, [bundledTools, workspaceConfig.toolInstallSource, workspaceConfig.toolName]);

  useEffect(() => {
    const refreshChannelFields = async () => {
      try {
        const launchConfig = await invokeTauri<LaunchEnvConfig>("get_launch_env_config");
        const resolved = resolveChannelFormFromLaunchConfig(launchConfig, workspaceConfig.channelType);
        setWorkspaceConfig((current) => ({
          ...current,
          channelAppIdOrToken: resolved.appIdOrToken,
          channelAppSecret: resolved.appSecret,
          channelVerificationToken: resolved.verificationToken,
        }));
      } catch {}
    };
    void refreshChannelFields();
  }, [workspaceConfig.channelType]);

  useEffect(() => {
    if (activeSubtab !== "logs") {
      return;
    }
    void fetchIronclawLogs();
    const timer = window.setInterval(() => {
      void fetchIronclawLogs();
    }, 1000);
    return () => window.clearInterval(timer);
  }, [activeSubtab]);

  const statusPillClassName = useMemo(
    () =>
      runtimeStatus.running
        ? isDark
          ? "rounded-full border border-emerald-800 bg-emerald-950 px-2 py-0.5 text-xs text-emerald-300"
          : "rounded-full border border-emerald-200 bg-emerald-50 px-2 py-0.5 text-xs text-emerald-700"
        : isDark
          ? "rounded-full border border-slate-700 bg-slate-900 px-2 py-0.5 text-xs text-slate-300"
          : "rounded-full border border-slate-200 bg-white px-2 py-0.5 text-xs text-slate-600",
    [runtimeStatus.running, isDark],
  );

  const tabClassName = (selected: boolean) =>
    selected
      ? isDark
        ? "rounded-lg border border-blue-700 bg-blue-950 px-3 py-1.5 text-sm text-blue-300"
        : "rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-sm text-blue-700"
      : isDark
        ? "rounded-lg border border-transparent bg-transparent px-3 py-1.5 text-sm text-slate-300"
        : "rounded-lg border border-transparent bg-transparent px-3 py-1.5 text-sm text-slate-700";

  const subtabClassName = (selected: boolean) =>
    selected
      ? isDark
        ? "rounded-lg border border-blue-700 bg-blue-950 px-3 py-1.5 text-sm text-blue-300"
        : "rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-sm text-blue-700"
      : isDark
        ? "rounded-lg border border-slate-700 bg-slate-900 px-3 py-1.5 text-sm text-slate-300"
        : "rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-sm text-slate-700";

  const inputClassName = isDark
    ? "w-full rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 outline-none focus:border-blue-500"
    : "w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 outline-none focus:border-blue-300";
  const buttonClassName = isDark
    ? "rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 hover:bg-slate-800"
    : "rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 hover:bg-slate-50";
  const cardClassName = isDark ? "rounded-xl border border-slate-700 bg-slate-900 p-4" : "rounded-xl border border-slate-200 bg-white p-4";
  const labelClassName = isDark ? "flex flex-col gap-1.5 text-sm text-slate-300" : "flex flex-col gap-1.5 text-sm text-slate-700";
  const textMutedClassName = isDark ? "text-sm text-slate-400" : "text-sm text-slate-600";
  const pageClassName = isDark ? "min-h-screen bg-slate-950 p-5 text-slate-100" : "min-h-screen bg-slate-100 p-5 text-slate-900";
  const headerClassName = isDark
    ? "flex items-center justify-between rounded-xl border border-slate-700 bg-slate-900 px-4 py-3"
    : "flex items-center justify-between rounded-xl border border-slate-200 bg-white px-4 py-3";
  const versionClassName = isDark ? "text-xs text-slate-400" : "text-xs text-slate-500";
  const logsClassName = isDark
    ? "max-h-[480px] overflow-auto rounded-lg border border-slate-700 bg-slate-950 p-3 text-xs text-slate-100"
    : "max-h-[480px] overflow-auto rounded-lg border border-slate-200 bg-slate-950 p-3 text-xs text-slate-100";
  const switchTrackClassName = isDark
    ? "relative inline-flex h-6 w-11 items-center rounded-full bg-blue-600 transition-colors"
    : "relative inline-flex h-6 w-11 items-center rounded-full bg-slate-300 transition-colors";
  const switchThumbClassName = isDark
    ? "inline-block h-5 w-5 translate-x-5 rounded-full bg-white transition-transform"
    : "inline-block h-5 w-5 translate-x-1 rounded-full bg-white transition-transform";
  const switchLabelClassName = isDark ? "text-sm text-slate-300" : "text-sm text-slate-700";
  const channelFieldMeta = getChannelFieldMeta(workspaceConfig.channelType);
  const previewTunnelNodeId = workspaceConfig.tunnelNodeId.trim().length >= 32 ? workspaceConfig.tunnelNodeId.trim() : "pc";
  const previewTunnelAddress = `https://workbot.axiayun.com/proxy/${workspaceConfig.tunnelUsername.trim() || "xm"}/${previewTunnelNodeId}/webhook/${channelTypeToSlug(workspaceConfig.channelType)}?secret=${workspaceConfig.channelVerificationToken.trim() || "<VerificationToken>"}`;

  return (
    <main className={pageClassName}>
      <div className="mx-auto flex max-w-7xl flex-col gap-3">
        <header className={headerClassName}>
          <div className="flex items-baseline gap-2">
            <strong>IronClaw Desktop</strong>
            <span className={versionClassName}>v0.1.0</span>
          </div>
          <nav className="flex items-center gap-2">
            <button
              type="button"
              className="inline-flex items-center gap-2 rounded-lg px-2 py-1"
              onClick={() => setUiMode(isDark ? "light" : "dark")}
              role="switch"
              aria-checked={isDark}
              aria-label="切换暗黑模式"
            >
              <span className={switchTrackClassName}>
                <span className={switchThumbClassName} />
              </span>
              <span className={switchLabelClassName}>暗黑模式</span>
            </button>
            <button type="button" className={tabClassName(activeTab === "workspace")} onClick={() => setActiveTab("workspace")}>
              工作台
            </button>
            <button type="button" className={tabClassName(activeTab === "employees")} onClick={() => setActiveTab("employees")}>
              数字员工
            </button>
          </nav>
        </header>

        {activeTab === "workspace" && (
          <section className="flex flex-col gap-3">
            <h1 className="text-2xl font-semibold">工作台</h1>

            <article className={cardClassName}>
              <div className="mb-2 flex flex-wrap items-center justify-between gap-3">
                <div className="flex items-center gap-2">
                  <strong>IronClaw 运行时</strong>
                  <span className={statusPillClassName}>{runtimeStatus.running ? "运行中" : "未运行"}</span>
                </div>
                <div className="flex flex-wrap gap-2">
                  <button type="button" className={buttonClassName} onClick={() => void startIronclawRun()}>
                    运行
                  </button>
                  <button type="button" className={buttonClassName} onClick={() => void stopIronclawRun()}>
                    停止
                  </button>
                  <button type="button" className={buttonClassName} onClick={() => void openConsoleLink()}>
                    打开控制台
                  </button>
                  <button type="button" className={buttonClassName} onClick={() => void saveTunnelConfig()}>
                    启动通道命令
                  </button>
                </div>
              </div>
              <div className="mb-2 flex flex-wrap gap-2">
                <input
                  type="password"
                  className={`${inputClassName} min-w-72 flex-1`}
                  placeholder="输入 GATEWAY_AUTH_TOKEN"
                  value={workspaceConfig.gatewayToken}
                  onChange={(event) => updateConfig("gatewayToken", event.target.value)}
                />
                <button type="button" className={buttonClassName} onClick={() => void applyGatewayToken()}>
                  应用 Token
                </button>
              </div>
              <p className={textMutedClassName}>{runtimeResult}</p>
            </article>

            <div className="flex gap-2">
              <button type="button" className={subtabClassName(activeSubtab === "models")} onClick={() => setActiveSubtab("models")}>
                模型
              </button>
              <button type="button" className={subtabClassName(activeSubtab === "channels")} onClick={() => setActiveSubtab("channels")}>
                通道
              </button>
              <button type="button" className={subtabClassName(activeSubtab === "tools")} onClick={() => setActiveSubtab("tools")}>
                工具
              </button>
              <button type="button" className={subtabClassName(activeSubtab === "skills")} onClick={() => setActiveSubtab("skills")}>
                技能
              </button>
              <button type="button" className={subtabClassName(activeSubtab === "logs")} onClick={() => setActiveSubtab("logs")}>
                日志
              </button>
            </div>

            {activeSubtab === "models" && (
              <article className={cardClassName}>
                <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                  <h2 className="text-lg font-semibold">模型配置</h2>
                  <div className="flex gap-2">
                    <button type="button" className={buttonClassName} onClick={() => void saveModelAndRestart()}>
                      保存并重启
                    </button>
                  </div>
                </div>
                <form className="grid grid-cols-1 gap-3 md:grid-cols-2">
                  <label className={labelClassName}>
                    协议类型
                    <select
                      className={inputClassName}
                      value={workspaceConfig.modelProtocol}
                      onChange={(event) => updateConfig("modelProtocol", event.target.value)}
                    >
                      <option>openai-completions</option>
                      <option>anthropic-messages</option>
                    </select>
                  </label>
                  <label className={labelClassName}>
                    Provider ID
                    <input
                      className={inputClassName}
                      placeholder="例如：proxy"
                      value={workspaceConfig.modelProviderId}
                      onChange={(event) => updateConfig("modelProviderId", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Base URL
                    <input
                      className={inputClassName}
                      placeholder="https://your-endpoint/v1"
                      value={workspaceConfig.modelBaseUrl}
                      onChange={(event) => updateConfig("modelBaseUrl", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    API Key
                    <input
                      type="password"
                      className={inputClassName}
                      placeholder="输入 API Key"
                      value={workspaceConfig.modelApiKey}
                      onChange={(event) => updateConfig("modelApiKey", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    模型 ID
                    <input
                      className={inputClassName}
                      placeholder="例如：deepseek-chat"
                      value={workspaceConfig.modelId}
                      onChange={(event) => updateConfig("modelId", event.target.value)}
                    />
                  </label>
                </form>
              </article>
            )}

            {activeSubtab === "channels" && (
              <article className={cardClassName}>
                <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                  <h2 className="text-lg font-semibold">通道管理</h2>
                  <button type="button" className={buttonClassName} onClick={() => void saveChannelConfig()}>
                    保存通道
                  </button>
                </div>
                <form className="grid grid-cols-1 gap-3 md:grid-cols-2">
                  <label className={labelClassName}>
                    通道类型
                    <select
                      className={inputClassName}
                      value={workspaceConfig.channelType}
                      onChange={(event) => updateConfig("channelType", event.target.value)}
                    >
                      <option>Feishu</option>
                      <option>Telegram</option>
                      <option>Slack</option>
                      <option>Discord</option>
                      <option>WhatsApp</option>
                    </select>
                  </label>
                  <label className={labelClassName}>
                    {channelFieldMeta.firstLabel}
                    <input
                      className={inputClassName}
                      placeholder={channelFieldMeta.firstPlaceholder}
                      value={workspaceConfig.channelAppIdOrToken}
                      onChange={(event) => updateConfig("channelAppIdOrToken", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    {channelFieldMeta.secondLabel}
                    <input
                      type="password"
                      className={inputClassName}
                      placeholder={channelFieldMeta.secondPlaceholder}
                      value={workspaceConfig.channelAppSecret}
                      disabled={Boolean(channelFieldMeta.secondDisabled)}
                      onChange={(event) => updateConfig("channelAppSecret", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    {channelFieldMeta.thirdLabel}
                    <input
                      className={inputClassName}
                      placeholder={channelFieldMeta.thirdPlaceholder}
                      value={workspaceConfig.channelVerificationToken}
                      disabled={Boolean(channelFieldMeta.thirdDisabled)}
                      onChange={(event) => updateConfig("channelVerificationToken", event.target.value)}
                    />
                  </label>
                </form>
                <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-2">
                  <label className={labelClassName}>
                    Tunnel 用户名
                    <input
                      className={inputClassName}
                      placeholder="例如：xm"
                      value={workspaceConfig.tunnelUsername}
                      onChange={(event) => updateConfig("tunnelUsername", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Tunnel 密码
                    <input
                      type="password"
                      className={inputClassName}
                      placeholder="输入 workbot 密码"
                      value={workspaceConfig.tunnelPassword}
                      onChange={(event) => updateConfig("tunnelPassword", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    node_id（至少 32 位）
                    <div className="flex gap-2">
                      <input
                        className={inputClassName}
                        placeholder="留空将自动随机生成"
                        value={workspaceConfig.tunnelNodeId}
                        onChange={(event) => updateConfig("tunnelNodeId", event.target.value)}
                      />
                      <button
                        type="button"
                        className={buttonClassName}
                        onClick={() => updateConfig("tunnelNodeId", generateNodeId())}
                      >
                        随机生成
                      </button>
                    </div>
                  </label>
                  <label className={labelClassName}>
                    启动命令
                    <input
                      className={inputClassName}
                      readOnly
                      value={workspaceConfig.tunnelStartCommand || "tunnel-client-darwin-arm64 -config ./workbot.yaml"}
                    />
                  </label>
                </div>
                <div className="mt-3 flex flex-col gap-2">
                  <p className={textMutedClassName}>运行目录：{workspaceConfig.tunnelWorkdir || "~/.ironclaw/tunnel"}</p>
                  <p className={textMutedClassName}>通道回调地址：{workspaceConfig.tunnelAddress || previewTunnelAddress}</p>
                  <div className="flex gap-2">
                    <button type="button" className={buttonClassName} onClick={() => void saveTunnelConfig()}>
                      保存并生成命令
                    </button>
                  </div>
                </div>
              </article>
            )}

            {activeSubtab === "tools" && (
              <article className={cardClassName}>
                <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                  <h2 className="text-lg font-semibold">工具管理</h2>
                  <button type="button" className={buttonClassName} onClick={() => void saveToolConfig()}>
                    安装工具
                  </button>
                </div>
                <form className="grid grid-cols-1 gap-3 md:grid-cols-2">
                  <label className={labelClassName}>
                    工具名称
                    {workspaceConfig.toolInstallSource.trim() ? (
                      <input
                        className={inputClassName}
                        placeholder="例如：github"
                        value={workspaceConfig.toolName}
                        onChange={(event) => updateConfig("toolName", event.target.value)}
                      />
                    ) : (
                      <select
                        className={inputClassName}
                        value={workspaceConfig.toolName}
                        onChange={(event) => updateConfig("toolName", event.target.value)}
                      >
                        {!workspaceConfig.toolName && <option value="">请选择内置工具</option>}
                        {bundledTools.map((tool) => (
                          <option key={tool} value={tool}>
                            {tool}
                          </option>
                        ))}
                      </select>
                    )}
                  </label>
                  <label className={labelClassName}>
                    安装来源
                    <input
                      className={inputClassName}
                      placeholder="留空=内置资源，或填写 https://xxx.zip"
                      value={workspaceConfig.toolInstallSource}
                      onChange={(event) => updateConfig("toolInstallSource", event.target.value)}
                    />
                  </label>
                </form>
              </article>
            )}

            {activeSubtab === "skills" && (
              <article className={cardClassName}>
                <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                  <h2 className="text-lg font-semibold">技能管理</h2>
                  <button type="button" className={buttonClassName} onClick={() => void saveSkillConfig()}>
                    安装技能
                  </button>
                </div>
                <form className="grid grid-cols-1 gap-3 md:grid-cols-2">
                  <label className={labelClassName}>
                    技能名称
                    <input
                      className={inputClassName}
                      placeholder="用于创建目录名，例如 my_skill"
                      value={workspaceConfig.skillName}
                      onChange={(event) => updateConfig("skillName", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    安装来源
                    <input
                      className={inputClassName}
                      placeholder="填写技能 URL，例如 https://xxx.zip 或 https://xxx/SKILL.md"
                      value={workspaceConfig.skillInstallSource}
                      onChange={(event) => updateConfig("skillInstallSource", event.target.value)}
                    />
                  </label>
                </form>
              </article>
            )}

            {activeSubtab === "logs" && (
              <article className={cardClassName}>
                <div className="mb-2 flex items-center justify-between gap-2">
                  <h2 className="text-lg font-semibold">Ironclaw 日志</h2>
                  <button type="button" className={buttonClassName} onClick={() => void fetchIronclawLogs()}>
                    刷新
                  </button>
                </div>
                <pre className={logsClassName}>
                  {logsContent || "暂无日志"}
                </pre>
              </article>
            )}
          </section>
        )}

        {activeTab === "employees" && (
          <section className="flex flex-col gap-3">
            <h1 className="text-2xl font-semibold">数字员工</h1>
            <article className={cardClassName}>
              <p className={textMutedClassName}>从远程目录搜索岗位并一键安装 skills / tools / MCP。</p>
            </article>
          </section>
        )}

      </div>
    </main>
  );
}

export default App;
