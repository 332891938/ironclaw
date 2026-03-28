import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

type MainTab = "workspace" | "employees" | "console";
type WorkspaceSubtab = "models" | "channels" | "tools" | "logs";

type IronclawRuntimeInfo = {
  configuredPath: string | null;
  discoveredPath: string | null;
  bundledCandidatePath: string | null;
  version: string | null;
  available: boolean;
  message: string;
};

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
  toolName: string;
  toolInstallSource: string;
  toolCommand: string;
  toolEnv: string;
};

const WORKSPACE_CONFIG_STORAGE_KEY = "ironclaw.workspace.config.v1";

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
  toolName: "",
  toolInstallSource: "",
  toolCommand: "",
  toolEnv: "",
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
  const [runtimeResult, setRuntimeResult] = useState("检测中...");
  const [gatewayUrl, setGatewayUrl] = useState("http://127.0.0.1:3000/");
  const [workspaceConfig, setWorkspaceConfig] = useState<WorkspaceConfig>(readStoredConfig);
  const [logsContent, setLogsContent] = useState("");

  useEffect(() => {
    localStorage.setItem(WORKSPACE_CONFIG_STORAGE_KEY, JSON.stringify(workspaceConfig));
  }, [workspaceConfig]);

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

  const detectIronclawRuntime = async () => {
    setRuntimeResult("检测中...");
    try {
      const info = await invokeTauri<IronclawRuntimeInfo>("detect_ironclaw_runtime");
      setRuntimeResult(info.available ? "已安装" : "未安装");
    } catch (error) {
      setRuntimeResult(`检测失败: ${String(error)}`);
    }
  };

  const applyGatewayToken = async () => {
    setRuntimeResult("正在应用 GATEWAY_AUTH_TOKEN...");
    try {
      const info = await invokeTauri<GatewayInfo>("set_gateway_auth_token", {
        token: workspaceConfig.gatewayToken,
      });
      setGatewayUrl(info.url);
      setRuntimeResult(info.hasToken ? "Token 已应用" : "已清空自定义 Token，使用默认配置");
      setWorkspaceConfig((current) => ({ ...current, gatewayToken: info.token ?? "" }));
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

  const openConsoleWindow = async () => {
    setRuntimeResult("正在应用内打开控制台...");
    try {
      await invokeTauri("open_console_window", { url: gatewayUrl });
      setRuntimeResult("已在应用内打开控制台窗口");
    } catch (error) {
      setRuntimeResult(`应用内窗口打开失败，已回退浏览器: ${String(error)}`);
      await openConsoleLink();
    }
  };

  useEffect(() => {
    void refreshGatewayInfo();
    void refreshIronclawRunStatus();
    void detectIronclawRuntime();
    void loadLaunchEnvConfig();
    void fetchIronclawLogs();
  }, []);

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

  useEffect(() => {
    if (activeTab === "console") {
      void openConsoleWindow();
    }
  }, [activeTab]);

  const statusPillClassName = useMemo(
    () =>
      runtimeStatus.running
        ? "rounded-full border border-emerald-200 bg-emerald-50 px-2 py-0.5 text-xs text-emerald-700"
        : "rounded-full border border-slate-200 bg-white px-2 py-0.5 text-xs text-slate-600",
    [runtimeStatus.running],
  );

  const tabClassName = (selected: boolean) =>
    selected
      ? "rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-sm text-blue-700"
      : "rounded-lg border border-transparent bg-transparent px-3 py-1.5 text-sm text-slate-700";

  const subtabClassName = (selected: boolean) =>
    selected
      ? "rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-sm text-blue-700"
      : "rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-sm text-slate-700";

  const inputClassName =
    "w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 outline-none focus:border-blue-300";
  const buttonClassName =
    "rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 hover:bg-slate-50";
  const cardClassName = "rounded-xl border border-slate-200 bg-white p-4";
  const labelClassName = "flex flex-col gap-1.5 text-sm text-slate-700";
  const channelFieldMeta = getChannelFieldMeta(workspaceConfig.channelType);

  return (
    <main className="min-h-screen bg-slate-100 p-5 text-slate-900">
      <div className="mx-auto flex max-w-7xl flex-col gap-3">
        <header className="flex items-center justify-between rounded-xl border border-slate-200 bg-white px-4 py-3">
          <div className="flex items-baseline gap-2">
            <strong>IronClaw Desktop</strong>
            <span className="text-xs text-slate-500">v0.1.0</span>
          </div>
          <nav className="flex gap-2">
            <button type="button" className={tabClassName(activeTab === "workspace")} onClick={() => setActiveTab("workspace")}>
              工作台
            </button>
            <button type="button" className={tabClassName(activeTab === "employees")} onClick={() => setActiveTab("employees")}>
              数字员工
            </button>
            <button type="button" className={tabClassName(activeTab === "console")} onClick={() => setActiveTab("console")}>
              控制台
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
                  <button type="button" className={buttonClassName} onClick={() => void detectIronclawRuntime()}>
                    检测
                  </button>
                  <button type="button" className={buttonClassName} onClick={() => void openConsoleLink()}>
                    打开控制台
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
              <p className="text-sm text-slate-600">{runtimeResult}</p>
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
              <button type="button" className={subtabClassName(activeSubtab === "logs")} onClick={() => setActiveSubtab("logs")}>
                日志
              </button>
            </div>

            {activeSubtab === "models" && (
              <article className={cardClassName}>
                <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                  <h2 className="text-lg font-semibold">模型配置</h2>
                  <div className="flex gap-2">
                    <button type="button" className={buttonClassName} onClick={() => void saveLaunchEnvConfig(true)}>
                      保存
                    </button>
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
              </article>
            )}

            {activeSubtab === "tools" && (
              <article className={cardClassName}>
                <h2 className="mb-2 text-lg font-semibold">工具管理</h2>
                <form className="grid grid-cols-1 gap-3 md:grid-cols-2">
                  <label className={labelClassName}>
                    工具名称
                    <input
                      className={inputClassName}
                      placeholder="例如：github"
                      value={workspaceConfig.toolName}
                      onChange={(event) => updateConfig("toolName", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    安装来源
                    <input
                      className={inputClassName}
                      placeholder="registry / URL / 本地目录"
                      value={workspaceConfig.toolInstallSource}
                      onChange={(event) => updateConfig("toolInstallSource", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    执行命令
                    <input
                      className={inputClassName}
                      placeholder="工具入口命令"
                      value={workspaceConfig.toolCommand}
                      onChange={(event) => updateConfig("toolCommand", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    环境变量
                    <input
                      className={inputClassName}
                      placeholder="KEY=VALUE"
                      value={workspaceConfig.toolEnv}
                      onChange={(event) => updateConfig("toolEnv", event.target.value)}
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
                <pre className="max-h-[480px] overflow-auto rounded-lg border border-slate-200 bg-slate-950 p-3 text-xs text-slate-100">
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
              <p className="text-sm text-slate-600">从远程目录搜索岗位并一键安装 skills / tools / MCP。</p>
            </article>
          </section>
        )}

        {activeTab === "console" && (
          <section className="flex flex-col gap-3">
            <h1 className="text-2xl font-semibold">控制台</h1>
            <article className={cardClassName}>
              <div className="mb-2 flex gap-2">
                <button type="button" className={buttonClassName} onClick={() => void openConsoleWindow()}>
                  应用内窗口打开
                </button>
                <button type="button" className={buttonClassName} onClick={() => void openConsoleLink()}>
                  浏览器打开
                </button>
              </div>
              <p className="text-sm text-slate-600">
                切换到该页会自动在应用内窗口打开控制台，若未弹出可手动点击“应用内窗口打开”。
              </p>
            </article>
          </section>
        )}
      </div>
    </main>
  );
}

export default App;
