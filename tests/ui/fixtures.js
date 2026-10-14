const { test: base, expect } = require("@playwright/test");
const { version: appVersion } = require("../../tauri.conf.json");

const test = base.extend({
  page: async ({ page }, use, testInfo) => {
    const diagnostics = { pageErrors: [], consoleErrors: [], failedRequests: [], httpErrors: [] };
    page.on("pageerror", (error) => diagnostics.pageErrors.push(error.message));
    page.on("console", (message) => {
      if (message.type() === "error") diagnostics.consoleErrors.push(message.text());
    });
    page.on("requestfailed", (request) => {
      diagnostics.failedRequests.push({
        url: request.url(),
        error: request.failure()?.errorText ?? "unknown request failure",
      });
    });
    page.on("response", (response) => {
      if (response.status() >= 400) {
        diagnostics.httpErrors.push({ status: response.status(), url: response.url() });
      }
    });

    await use(page);

    const hasBrowserDiagnostics = Object.values(diagnostics).some((items) => items.length > 0);
    if (testInfo.status === "failed" || hasBrowserDiagnostics) {
      await testInfo.attach("browser-diagnostics.json", {
        body: Buffer.from(JSON.stringify(diagnostics, null, 2)),
        contentType: "application/json",
      });
    }
    expect(diagnostics.pageErrors, "uncaught browser exceptions").toEqual([]);
    expect(diagnostics.consoleErrors, "browser console errors").toEqual([]);
    expect(diagnostics.failedRequests, "failed browser requests").toEqual([]);
    expect(diagnostics.httpErrors, "HTTP responses with an error status").toEqual([]);
  },
});

function quota(remaining, windowDurationMins) {
  return {
    remainingFraction: remaining / 100,
    remainingPercent: String(remaining),
    resetsAt: 1_792_000_000,
    windowDurationMins,
    usedPercent: String(100 - remaining),
  };
}

function usageState(remaining5h = 72, remainingWeekly = 64, status = "online") {
  return {
    status,
    fiveHour: remaining5h === null ? null : quota(remaining5h, 300),
    weekly: remainingWeekly === null ? null : quota(remainingWeekly, 10_080),
  };
}

async function installTauriMock(page, options = {}) {
  await page.addInitScript((config) => {
    const defaults = {
      threshold5h: 20,
      thresholdWeekly: 20,
      uiScale: 100,
      alwaysOnTop: true,
      closeAction: "ask",
      language: "en",
      ultraCompact: false,
    };
    let settings;
    try {
      settings = JSON.parse(localStorage.getItem("codex-mystats-ui-settings"));
    } catch {
      settings = null;
    }
    if (!settings || typeof settings !== "object") {
      settings = config.settings && typeof config.settings === "object"
        ? { ...config.settings }
        : { ...defaults };
    }
    let currentUsage = config.usageState ?? {
      status: "online",
      fiveHour: {
        windowDurationMins: 300,
        usedPercent: "28",
        remainingPercent: "72",
        remainingFraction: 0.72,
        resetsAt: 1_792_000_000,
      },
      weekly: {
        windowDurationMins: 10_080,
        usedPercent: "36",
        remainingPercent: "64",
        remainingFraction: 0.64,
        resetsAt: 1_792_000_000,
      },
    };
    const listeners = new Map();
    const calls = [];
    const failures = new Set(config.failures ?? []);

    window.__mockCalls = calls;
    window.__mockFailures = failures;
    window.__mockCurrentUsage = currentUsage;
    window.__mockGetSettings = () => ({ ...settings });
    window.__mockEmitUsage = (payload) => {
      currentUsage = payload;
      window.__mockCurrentUsage = payload;
      for (const listener of listeners.get("usage-state") ?? []) listener({ payload });
    };

    window.__TAURI__ = {
      core: {
        invoke: async (command, args = {}) => {
          calls.push({ command, args });
          if (failures.has(command)) throw new Error(`Mock failure: ${command}`);
          switch (command) {
            case "get_app_version":
              return config.versionFailure
                ? Promise.reject(new Error("Version unavailable"))
                : (config.appVersion ?? config.buildVersion);
            case "get_usage_state":
              if (config.usageFailure) throw new Error("Usage limits unavailable");
              return currentUsage;
            case "load_settings":
              if (config.loadFailure) throw new Error("Settings unavailable");
              return { ...settings };
            case "save_settings":
              if (config.saveFailure) throw new Error("Settings could not be saved");
              settings = { ...args.settings };
              localStorage.setItem("codex-mystats-ui-settings", JSON.stringify(settings));
              return { ...settings };
            case "set_ultra_compact":
            case "set_always_on_top":
              return undefined;
            case "minimize_panel":
            case "quit_app":
              return undefined;
            default:
              throw new Error(`Unexpected Tauri command: ${command}`);
          }
        },
      },
      event: {
        listen: async (eventName, callback) => {
          if (config.listenFailure) throw new Error("Event listener unavailable");
          const eventListeners = listeners.get(eventName) ?? [];
          eventListeners.push(callback);
          listeners.set(eventName, eventListeners);
          if (eventName === "usage-state" && config.usageEventOnListen) {
            window.__mockEmitUsage(config.usageEventOnListen);
          }
          return () => {};
        },
      },
      window: {
        getCurrentWindow: () => ({
          startResizeDragging: async () => {},
        }),
      },
    };
  }, { buildVersion: appVersion, ...options });

  await page.goto("/");
  if (!options.loadFailure) {
    await page.waitForFunction(() => {
      if (window.__mockCalls?.some((call) => call.command === "set_always_on_top")) return true;
      const status = document.querySelector("#connection-message")?.textContent;
      return status && !["Connecting…", "Conectando…", "Updating limits…", "Actualizando límites…"].includes(status);
    });
  }
}

module.exports = { test, expect, installTauriMock, usageState, appVersion };
