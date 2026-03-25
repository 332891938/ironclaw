import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

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
  ipc?: unknown;
  postMessage?: unknown;
};

const WORKSPACE_CONFIG_STORAGE_KEY = "ironclaw.workspace.config.v1";

function getTauriInternals(): TauriInternals | undefined {
  return (window as typeof window & { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
}

function buildBridgeStateText() {
  const internals = getTauriInternals();
  const hasInvoke = Boolean(internals?.invoke);
  const hasIpc = Boolean(internals?.ipc);
  const hasPostMessage = Boolean(internals?.postMessage);
  const hasWindowIpc = Boolean((window as typeof window & { ipc?: unknown }).ipc);
  return `origin=${location.origin}, invoke=${hasInvoke}, ipc=${hasIpc}, postMessage=${hasPostMessage}, window.ipc=${hasWindowIpc}`;
}

async function waitForTauriBridge(timeoutMs = 8000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const internals = getTauriInternals();
    if (internals?.invoke) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Tauri IPC 未注入: ${buildBridgeStateText()}`);
}

async function invokeTauri<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  await waitForTauriBridge();
  return invoke<T>(cmd, args);
}

window.addEventListener("DOMContentLoaded", () => {
  const tabs = Array.from(document.querySelectorAll<HTMLButtonElement>(".top-tab"));
  const panels = Array.from(document.querySelectorAll<HTMLElement>(".tab-panel"));
  const subtabs = Array.from(document.querySelectorAll<HTMLButtonElement>(".subtab"));
  const subpanels = Array.from(document.querySelectorAll<HTMLElement>(".subpanel"));
  const detectBtn = document.querySelector<HTMLButtonElement>("#detect-ironclaw-btn");
  const startBtn = document.querySelector<HTMLButtonElement>("#start-ironclaw-btn");
  const stopBtn = document.querySelector<HTMLButtonElement>("#stop-ironclaw-btn");
  const applyGatewayTokenBtn = document.querySelector<HTMLButtonElement>("#apply-gateway-token-btn");
  const openConsoleLinkBtn = document.querySelector<HTMLButtonElement>("#open-console-link-btn");
  const openConsoleLinkPanelBtn = document.querySelector<HTMLButtonElement>("#open-console-link-panel-btn");
  const openConsoleWindowBtn = document.querySelector<HTMLButtonElement>("#open-console-window-btn");
  const gatewayTokenInput = document.querySelector<HTMLInputElement>("#gateway-token-input");
  const workspaceConfigFields = Array.from(
    document.querySelectorAll<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>(
      "#panel-workspace [data-config-key]",
    ),
  );
  const runtimeResult = document.querySelector<HTMLElement>("#ironclaw-runtime-result");
  const runtimeStatusPill = document.querySelector<HTMLElement>("#runtime-status-pill");
  let currentGatewayUrl = "http://127.0.0.1:3000/";

  function saveWorkspaceConfig() {
    const config = workspaceConfigFields.reduce<Record<string, string>>((acc, field) => {
      const key = field.dataset.configKey;
      if (key) {
        acc[key] = field.value;
      }
      return acc;
    }, {});
    localStorage.setItem(WORKSPACE_CONFIG_STORAGE_KEY, JSON.stringify(config));
  }

  function restoreWorkspaceConfig() {
    const raw = localStorage.getItem(WORKSPACE_CONFIG_STORAGE_KEY);
    if (!raw) {
      return;
    }
    const parsed = JSON.parse(raw) as Record<string, string>;
    workspaceConfigFields.forEach((field) => {
      const key = field.dataset.configKey;
      if (!key) {
        return;
      }
      if (Object.prototype.hasOwnProperty.call(parsed, key)) {
        field.value = parsed[key] ?? "";
      }
    });
  }

  function activateTab(tabId: string) {
    tabs.forEach((tab) => {
      const selected = tab.dataset.tab === tabId;
      tab.classList.toggle("active", selected);
      tab.setAttribute("aria-selected", String(selected));
    });

    panels.forEach((panel) => {
      panel.classList.toggle("active", panel.id === `panel-${tabId}`);
    });
  }

  function activateSubtab(subtabId: string) {
    subtabs.forEach((subtab) => {
      const selected = subtab.dataset.subtab === subtabId;
      subtab.classList.toggle("active", selected);
      subtab.setAttribute("aria-selected", String(selected));
    });

    subpanels.forEach((panel) => {
      panel.classList.toggle("active", panel.id === `subpanel-${subtabId}`);
    });
  }

  async function detectIronclawRuntime() {
    if (!runtimeResult) {
      return;
    }
    runtimeResult.textContent = "检测中...";
    try {
      const info = await invokeTauri<IronclawRuntimeInfo>("detect_ironclaw_runtime");
      runtimeResult.textContent = info.available ? "已安装" : "未安装";
    } catch (error) {
      runtimeResult.textContent = `检测失败: ${String(error)}`;
    }
  }

  function setRuntimeStatus(status: IronclawRunStatus) {
    if (!runtimeStatusPill) {
      return;
    }
    runtimeStatusPill.textContent = status.running ? "运行中" : "未运行";
    runtimeStatusPill.classList.toggle("running", status.running);
  }

  function setGatewayAddress(url: string) {
    currentGatewayUrl = url;
  }

  async function refreshGatewayInfo() {
    try {
      const info = await invokeTauri<GatewayInfo>("get_gateway_info");
      setGatewayAddress(info.url);
      if (gatewayTokenInput) {
        gatewayTokenInput.value = info.token ?? "";
      }
      saveWorkspaceConfig();
    } catch {
      setGatewayAddress("http://127.0.0.1:3000/");
    }
  }

  async function applyGatewayToken() {
    if (!runtimeResult || !gatewayTokenInput) {
      return;
    }
    runtimeResult.textContent = "正在应用 GATEWAY_AUTH_TOKEN...";
    try {
      const info = await invokeTauri<GatewayInfo>("set_gateway_auth_token", {
        token: gatewayTokenInput.value,
      });
      setGatewayAddress(info.url);
      saveWorkspaceConfig();
      runtimeResult.textContent = info.hasToken ? "Token 已应用" : "已清空自定义 Token，使用默认配置";
    } catch (error) {
      runtimeResult.textContent = `应用 Token 失败: ${String(error)}`;
    }
  }

  async function refreshIronclawRunStatus() {
    try {
      const status = await invokeTauri<IronclawRunStatus>("get_ironclaw_run_status");
      setRuntimeStatus(status);
    } catch {
      setRuntimeStatus({ running: false, pid: null, message: "状态读取失败" });
    }
  }

  async function startIronclawRun() {
    if (!runtimeResult) {
      return;
    }
    runtimeResult.textContent = "正在启动 ironclaw run...";
    try {
      const status = await invokeTauri<IronclawRunStatus>("start_ironclaw_run");
      await refreshGatewayInfo();
      runtimeResult.textContent = status.message;
      setRuntimeStatus(status);
    } catch (error) {
      runtimeResult.textContent = `启动失败: ${String(error)}`;
    }
  }

  async function stopIronclawRun() {
    if (!runtimeResult) {
      return;
    }
    runtimeResult.textContent = "正在停止 ironclaw run...";
    try {
      const status = await invokeTauri<IronclawRunStatus>("stop_ironclaw_run");
      runtimeResult.textContent = status.message;
      setRuntimeStatus(status);
    } catch (error) {
      runtimeResult.textContent = `停止失败: ${String(error)}`;
    }
  }

  async function openConsoleLink() {
    await openUrl(currentGatewayUrl);
  }

  async function openConsoleWindow() {
    if (runtimeResult) {
      runtimeResult.textContent = "正在应用内打开控制台...";
    }
    try {
      await invokeTauri("open_console_window", { url: currentGatewayUrl });
      if (runtimeResult) {
        runtimeResult.textContent = "已在应用内打开控制台窗口";
      }
    } catch (error) {
      if (runtimeResult) {
        runtimeResult.textContent = `应用内窗口打开失败，已回退浏览器: ${String(error)}`;
      }
      await openConsoleLink();
    }
  }

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      const tabId = tab.dataset.tab;
      if (tabId) {
        activateTab(tabId);
        if (tabId === "console") {
          void openConsoleWindow();
        }
      }
    });
  });

  subtabs.forEach((subtab) => {
    subtab.addEventListener("click", () => {
      const subtabId = subtab.dataset.subtab;
      if (subtabId) {
        activateSubtab(subtabId);
      }
    });
  });

  detectBtn?.addEventListener("click", () => {
    void detectIronclawRuntime();
  });
  startBtn?.addEventListener("click", () => {
    void startIronclawRun();
  });
  stopBtn?.addEventListener("click", () => {
    void stopIronclawRun();
  });
  applyGatewayTokenBtn?.addEventListener("click", () => {
    void applyGatewayToken();
  });
  openConsoleLinkBtn?.addEventListener("click", () => {
    void openConsoleLink();
  });
  openConsoleLinkPanelBtn?.addEventListener("click", () => {
    void openConsoleLink();
  });
  openConsoleWindowBtn?.addEventListener("click", () => {
    void openConsoleWindow();
  });
  workspaceConfigFields.forEach((field) => {
    field.addEventListener("input", saveWorkspaceConfig);
    field.addEventListener("change", saveWorkspaceConfig);
  });

  activateTab("workspace");
  activateSubtab("models");
  restoreWorkspaceConfig();
  if (runtimeResult) {
    runtimeResult.textContent = "检测中...";
  }
  void refreshGatewayInfo();
  void refreshIronclawRunStatus();
  void detectIronclawRuntime();
});
