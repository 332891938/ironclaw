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
  const gatewayTokenInput = document.querySelector<HTMLInputElement>("#gateway-token-input");
  const runtimeResult = document.querySelector<HTMLElement>("#ironclaw-runtime-result");
  const runtimeStatusPill = document.querySelector<HTMLElement>("#runtime-status-pill");
  let currentGatewayUrl = "http://127.0.0.1:3000/";

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
      const info = await invoke<IronclawRuntimeInfo>("detect_ironclaw_runtime");
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
      const info = await invoke<GatewayInfo>("get_gateway_info");
      setGatewayAddress(info.url);
      if (gatewayTokenInput) {
        gatewayTokenInput.value = info.token ?? "";
      }
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
      const info = await invoke<GatewayInfo>("set_gateway_auth_token", {
        token: gatewayTokenInput.value,
      });
      setGatewayAddress(info.url);
      runtimeResult.textContent = info.hasToken ? "Token 已应用" : "已清空自定义 Token，使用默认配置";
    } catch (error) {
      runtimeResult.textContent = `应用 Token 失败: ${String(error)}`;
    }
  }

  async function refreshIronclawRunStatus() {
    try {
      const status = await invoke<IronclawRunStatus>("get_ironclaw_run_status");
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
      const status = await invoke<IronclawRunStatus>("start_ironclaw_run");
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
      const status = await invoke<IronclawRunStatus>("stop_ironclaw_run");
      runtimeResult.textContent = status.message;
      setRuntimeStatus(status);
    } catch (error) {
      runtimeResult.textContent = `停止失败: ${String(error)}`;
    }
  }

  async function openConsoleLink() {
    await openUrl(currentGatewayUrl);
  }

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      const tabId = tab.dataset.tab;
      if (tabId) {
        activateTab(tabId);
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

  activateTab("workspace");
  activateSubtab("models");
  void refreshGatewayInfo();
  void refreshIronclawRunStatus();
  void detectIronclawRuntime();
});
