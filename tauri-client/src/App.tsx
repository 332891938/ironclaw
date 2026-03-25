import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

type MainTab = "workspace" | "employees" | "console";
type WorkspaceSubtab = "models" | "channels" | "tools";

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
  modelDefaultRef: string;
  channelType: string;
  channelSourceUrl: string;
  channelAppIdOrToken: string;
  channelAppSecret: string;
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
  modelDefaultRef: "",
  channelType: "Feishu",
  channelSourceUrl: "",
  channelAppIdOrToken: "",
  channelAppSecret: "",
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

  useEffect(() => {
    localStorage.setItem(WORKSPACE_CONFIG_STORAGE_KEY, JSON.stringify(workspaceConfig));
  }, [workspaceConfig]);

  const updateConfig = (key: keyof WorkspaceConfig, value: string) => {
    setWorkspaceConfig((current) => ({ ...current, [key]: value }));
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
    setRuntimeResult("正在启动 ironclaw run...");
    try {
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
  }, []);

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
            </div>

            {activeSubtab === "models" && (
              <article className={cardClassName}>
                <h2 className="mb-2 text-lg font-semibold">模型配置</h2>
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
                  <label className={labelClassName}>
                    默认模型引用
                    <input
                      className={inputClassName}
                      placeholder="自动生成：provider/model"
                      value={workspaceConfig.modelDefaultRef}
                      onChange={(event) => updateConfig("modelDefaultRef", event.target.value)}
                    />
                  </label>
                </form>
              </article>
            )}

            {activeSubtab === "channels" && (
              <article className={cardClassName}>
                <h2 className="mb-2 text-lg font-semibold">通道管理</h2>
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
                    </select>
                  </label>
                  <label className={labelClassName}>
                    通道来源 URL
                    <input
                      className={inputClassName}
                      placeholder="WASM/registry 地址"
                      value={workspaceConfig.channelSourceUrl}
                      onChange={(event) => updateConfig("channelSourceUrl", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    App ID / Token
                    <input
                      className={inputClassName}
                      placeholder="输入通道鉴权字段"
                      value={workspaceConfig.channelAppIdOrToken}
                      onChange={(event) => updateConfig("channelAppIdOrToken", event.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    App Secret
                    <input
                      type="password"
                      className={inputClassName}
                      placeholder="输入通道密钥"
                      value={workspaceConfig.channelAppSecret}
                      onChange={(event) => updateConfig("channelAppSecret", event.target.value)}
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
