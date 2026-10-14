const fs = require("node:fs/promises");
const path = require("node:path");
const { test, expect, installTauriMock, usageState, appVersion } = require("./fixtures");

const previewDirectory = path.resolve("artifacts/ui-previews");

async function verifyVisibleLayout(page, expectedSize) {
  if (expectedSize.width === 300) {
    await expect.poll(() => page.locator("#five-hour-title").evaluate((node) => getComputedStyle(node).fontSize))
      .toBe("13px");
  }
  const layout = await page.evaluate(() => {
    const shellNode = document.querySelector(".app-shell");
    const titleNode = document.querySelector("#five-hour-title");
    const shell = shellNode.getBoundingClientRect();
    const cards = [...document.querySelectorAll(".quota-card")].map((card) => {
      const cardRect = card.getBoundingClientRect();
      const meter = card.querySelector(".quota-meter").getBoundingClientRect();
      const marker = card.querySelector(".threshold-marker").getBoundingClientRect();
      const segments = [...card.querySelectorAll(".threshold-marker i")].map((segment) => {
        const rect = segment.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, width: rect.width };
      });
      const threshold = Number.parseFloat(
        getComputedStyle(card.querySelector(".quota-meter")).getPropertyValue("--threshold-position"),
      );
      return {
        card: { left: cardRect.left, right: cardRect.right, top: cardRect.top, bottom: cardRect.bottom },
        cardBottom: cardRect.bottom,
        meterLeft: meter.left,
        meterRight: meter.right,
        meterTop: meter.top,
        meterBottom: meter.bottom,
        meterWidth: meter.width,
        fillWidth: card.querySelector(".meter-fill").getBoundingClientRect().width,
        markerCenter: marker.left + marker.width / 2,
        expectedMarkerCenter: meter.left + meter.width * threshold / 100,
        segments,
        title: card.querySelector(".quota-heading h1, .quota-heading h2").getBoundingClientRect(),
        percent: card.querySelector(".remaining-block strong").getBoundingClientRect(),
        remaining: card.querySelector(".remaining-block span").getBoundingClientRect(),
        reset: card.querySelector(".quota-reset").getBoundingClientRect(),
      };
    });
    return {
      viewport: { width: innerWidth, height: innerHeight },
      ultraCompact: shellNode.dataset.ultraCompact,
      titleFontSize: getComputedStyle(titleNode).fontSize,
      shell: { width: shell.width, height: shell.height, bottom: shell.bottom },
      document: {
        width: document.documentElement.scrollWidth,
        height: document.documentElement.scrollHeight,
      },
      cards,
    };
  });
  expect(layout.viewport).toEqual(expectedSize);
  expect(layout.ultraCompact).toBe(expectedSize.width === 300 ? "true" : "false");
  if (expectedSize.width === 300) expect(layout.titleFontSize, JSON.stringify(layout)).toBe("13px");
  expect(layout.shell.width).toBe(expectedSize.width);
  expect(layout.shell.height).toBe(expectedSize.height);
  expect(layout.document.width).toBeLessThanOrEqual(expectedSize.width);
  expect(layout.document.height).toBeLessThanOrEqual(expectedSize.height);
  for (const card of layout.cards) {
    expect(card.cardBottom, JSON.stringify(layout)).toBeLessThanOrEqual(layout.shell.bottom);
    expect(Math.abs(card.markerCenter - card.expectedMarkerCenter)).toBeLessThanOrEqual(1);
    expect(card.meterLeft).toBeGreaterThanOrEqual(card.card.left);
    expect(card.meterRight).toBeLessThanOrEqual(card.card.right);
    expect(card.meterTop).toBeGreaterThan(card.title.bottom);
    expect(card.meterBottom).toBeLessThan(card.reset.top);
    expect(card.reset.bottom).toBeLessThanOrEqual(layout.shell.bottom);
    expect(card.reset.right).toBeLessThanOrEqual(layout.shell.width);
    expect(card.segments).toHaveLength(3);
    for (const segment of card.segments) {
      expect(segment.width).toBe(2);
      expect(segment.bottom - segment.top).toBe(3);
    }
    expect(card.segments[1].top - card.segments[0].bottom).toBe(2);
    expect(card.segments[2].top - card.segments[1].bottom).toBe(2);
    if (expectedSize.width === 300) {
      expect(card.percent.right).toBeLessThanOrEqual(card.remaining.left + 0.5);
    }
  }
  return layout;
}

async function commandCalls(page, command) {
  return page.evaluate((name) => window.__mockCalls.filter((call) => call.command === name), command);
}

async function expectLimitsUnavailable(page, expectedSize) {
  for (const selector of ["#five-hour", "#weekly"]) {
    await expect(page.locator(`${selector}-percent`)).toHaveText("—");
    const meter = page.locator(`${selector}-progress`);
    await expect(meter).toHaveAttribute("data-heat", "unavailable");
    await expect.poll(() => page.locator(`${selector}-percent`).evaluate((node) => getComputedStyle(node).color))
      .toBe("rgb(105, 107, 116)");
    await expect(meter).not.toHaveAttribute("aria-valuenow", /.+/);
    await expect(meter).toHaveAttribute("aria-valuetext", "Usage limit unavailable.");
    const colors = await meter.evaluate((node) => {
      const style = getComputedStyle(node);
      return ["--water-deep", "--water-mid", "--water-light"]
        .map((property) => style.getPropertyValue(property).trim());
    });
    expect(colors).toEqual(["#4d535b", "#696b74", "#9696a0"]);
    await expect(page.locator(`${selector}-reset`)).toHaveText("Reset time unavailable");
    await expect(page.locator(`${selector}-card`)).toHaveClass(/quota--unavailable/);
  }

  return verifyVisibleLayout(page, expectedSize);
}

async function verifyCloseDialogLayout(page) {
  const layout = await page.locator("#close-dialog").evaluate((dialog) => {
    const dialogRect = dialog.getBoundingClientRect();
    const actions = dialog.querySelector(".close-dialog-actions").getBoundingClientRect();
    const buttons = [...dialog.querySelectorAll(".close-dialog-actions button")].map((button) => {
      const rect = button.getBoundingClientRect();
      return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
    });
    return {
      dialog: {
        inViewport: dialogRect.left >= 0 && dialogRect.top >= 0
          && dialogRect.right <= innerWidth && dialogRect.bottom <= innerHeight,
        noOverflow: dialog.scrollWidth <= dialog.clientWidth && dialog.scrollHeight <= dialog.clientHeight,
      },
      actions: { left: actions.left, right: actions.right, top: actions.top, bottom: actions.bottom },
      buttons,
    };
  });

  expect(layout.dialog).toEqual({ inViewport: true, noOverflow: true });
  expect(layout.buttons).toHaveLength(3);
  const rowTop = layout.buttons[0].top;
  for (const button of layout.buttons) {
    expect(Math.abs(button.top - rowTop)).toBeLessThanOrEqual(1);
    expect(button.left).toBeGreaterThanOrEqual(layout.actions.left);
    expect(button.right).toBeLessThanOrEqual(layout.actions.right);
    expect(button.bottom).toBeLessThanOrEqual(layout.actions.bottom);
  }
}

test("renders the normal panel at 360 by 230 without overflow", async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-10-14T15:00:00.000Z"));
  await installTauriMock(page, {});

  await expect(page.locator("#app-version-badge")).toHaveText(`v${appVersion}`);
  expect(await commandCalls(page, "get_app_version")).toHaveLength(1);
  await expect(page.locator(".app-name")).toHaveText("Codex MyStats");
  await expect(page.locator("#five-hour-percent")).toHaveText("72%");
  await expect(page.locator("#weekly-percent")).toHaveText("64%");
  const layout = await verifyVisibleLayout(page, { width: 360, height: 230 });
  expect(layout.cards[0].remaining.top).toBeGreaterThanOrEqual(layout.cards[0].percent.top);
});

test("edits whole-number thresholds and applies marker positions immediately", async ({ page }) => {
  await installTauriMock(page, {});
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("tab", { name: "Limits" }).click();
  await page.locator("#threshold-5h-value").fill("42");
  await page.locator("#threshold-weekly-value").fill("71");
  await expect(page.locator("#threshold-5h-range")).toHaveValue("42");
  await expect(page.locator("#threshold-weekly-range")).toHaveValue("71");
  await expect.poll(async () => page.evaluate(() => localStorage.getItem("codex-mystats-ui-settings")))
    .toContain('"threshold5h":42');

  await page.getByRole("button", { name: "Back" }).click();
  const layout = await verifyVisibleLayout(page, { width: 360, height: 230 });
  expect(layout.cards[0].expectedMarkerCenter).toBeCloseTo(
    layout.cards[0].meterLeft + layout.cards[0].meterWidth * 0.42,
    2,
  );
  expect(layout.cards[1].expectedMarkerCenter).toBeCloseTo(
    layout.cards[1].meterLeft + layout.cards[1].meterWidth * 0.71,
    2,
  );
});

test("rejects decimal and out-of-range thresholds while keeping the slider synchronized", async ({ page }) => {
  await installTauriMock(page, {});
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("tab", { name: "Limits" }).click();
  const thresholds = [
    { value: "#threshold-5h-value", slider: "#threshold-5h-range" },
    { value: "#threshold-weekly-value", slider: "#threshold-weekly-range" },
  ];
  for (const selectors of thresholds) {
    const threshold = page.locator(selectors.value);
    const slider = page.locator(selectors.slider);
    for (const control of [threshold, slider]) {
      await expect(control).toHaveAttribute("min", "0");
      await expect(control).toHaveAttribute("max", "100");
      await expect(control).toHaveAttribute("step", "1");
    }

    await threshold.fill("20.5");
    await expect(threshold).toHaveValue("20");
    await expect(slider).toHaveValue("20");
    await expect(page.locator("#limits-panel .save-message"))
      .toHaveText("Enter a whole number from 0 to 100.");

    await threshold.fill("101");
    await threshold.blur();
    await expect(threshold).toHaveValue("20");
    await expect(slider).toHaveValue("20");
  }

  await page.locator("#threshold-5h-range").evaluate((node) => {
    node.value = "0";
    node.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await expect(page.locator("#threshold-5h-value")).toHaveValue("0");
  await expect.poll(async () => page.evaluate(() => localStorage.getItem("codex-mystats-ui-settings")))
    .toContain('"threshold5h":0');
});

test("loads legacy preferences with defaults and preserves saved thresholds after restart", async ({ page }) => {
  await installTauriMock(page, { settings: { threshold5h: 37, thresholdWeekly: 62 } });
  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "false");
  await expect(page.locator("#ultra-compact-button")).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#language-en")).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("tab", { name: "Limits" }).click();
  await expect(page.locator("#threshold-5h-value")).toHaveValue("37");
  await expect(page.locator("#threshold-weekly-value")).toHaveValue("62");
  await page.locator("#threshold-5h-value").fill("38");
  await expect.poll(async () => page.evaluate(() => localStorage.getItem("codex-mystats-ui-settings")))
    .toContain('"threshold5h":38');
  await page.reload();

  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("tab", { name: "Limits" }).click();
  await expect(page.locator("#threshold-5h-value")).toHaveValue("38");
  await expect(page.locator("#threshold-weekly-value")).toHaveValue("62");
  await expect(page.locator("#language-en")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#ultra-compact-button")).toHaveAttribute("aria-pressed", "false");
});

test("renders ultracompact panel at 300 by 156 with inline remaining labels", async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-10-14T15:00:00.000Z"));
  await installTauriMock(page, { settings: { ultraCompact: true, threshold5h: 33, thresholdWeekly: 67 } });

  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "true");
  await expect(page.locator("#app-version-badge")).toHaveText(`v${appVersion}`);
  await expect(page.locator("#always-on-top-button")).toHaveCount(0);
  await page.setViewportSize({ width: 300, height: 156 });
  const layout = await verifyVisibleLayout(page, { width: 300, height: 156 });
  await expect.poll(async () => page.locator("#weekly-title").evaluate((node) => getComputedStyle(node).fontSize))
    .toBe("13px");
  expect(layout.shell.bottom - layout.cards[1].cardBottom).toBeLessThanOrEqual(9);
  for (const card of layout.cards) {
    expect(Math.abs(card.percent.top - card.remaining.top)).toBeLessThanOrEqual(2);
    expect(card.percent.left).toBeLessThan(card.remaining.left);
  }
});

test("captures deterministic normal and ultracompact panels", async ({ page }) => {
  await fs.mkdir(previewDirectory, { recursive: true });
  await page.clock.setFixedTime(new Date("2026-10-14T15:00:00.000Z"));
  await installTauriMock(page, { usageState: usageState(23, 73) });
  await expect(page.locator("#app-version-badge")).toHaveText(`v${appVersion}`);
  await page.locator(".app-shell").screenshot({
    path: path.join(previewDirectory, "normal.png"),
    animations: "disabled",
    caret: "hide",
  });

  await page.locator("#ultra-compact-button").click();
  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "true");
  await page.setViewportSize({ width: 300, height: 156 });
  await expect.poll(async () => page.locator("#five-hour-title").evaluate((node) => getComputedStyle(node).fontSize))
    .toBe("13px");
  await verifyVisibleLayout(page, { width: 300, height: 156 });
  await page.locator(".app-shell").screenshot({
    path: path.join(previewDirectory, "ultra-compact.png"),
    animations: "disabled",
    caret: "hide",
  });
});

test("uses ten heat color steps at each 10-point boundary", async ({ page }) => {
  await installTauriMock(page, {});
  // Measure the rendered target width, not the intermediate frames of the UI transition.
  await page.addStyleTag({ content: ".meter-fill { transition: none !important; }" });
  const palette = {
    "step-0": ["#873641", "#d35760", "#f49a91"],
    "step-1": ["#963a36", "#d75c55", "#f49d83"],
    "step-2": ["#a14330", "#dc624b", "#f3a175"],
    "step-3": ["#a84f2a", "#dc7040", "#f2a76f"],
    "step-4": ["#a4612a", "#d78a3f", "#eeb772"],
    "step-5": ["#92722f", "#cba142", "#e8c976"],
    "step-6": ["#7b7937", "#b4ac48", "#ded57c"],
    "step-7": ["#5e7a43", "#8db75b", "#c0dc86"],
    "step-8": ["#3d7750", "#63b583", "#a0dcb2"],
    "step-9": ["#267455", "#40b783", "#8de0b9"],
  };
  const cases = [];
  for (let step = 0; step < 10; step += 1) {
    cases.push([step * 10, `step-${step}`]);
    cases.push([step * 10 + 9, `step-${step}`]);
  }
  cases.push([100, "step-9"]);
  const modes = [
    { compact: false, size: { width: 360, height: 230 } },
    { compact: true, size: { width: 300, height: 156 } },
  ];
  for (const mode of modes) {
    if (mode.compact) {
      await page.getByRole("button", { name: "Turn on ultra-compact mode" }).click();
      await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "true");
      await page.setViewportSize(mode.size);
    }

    await verifyVisibleLayout(page, mode.size);
    for (const [remaining, heat] of cases) {
      await page.evaluate((value) => window.__mockEmitUsage({
        status: "online",
        fiveHour: {
          windowDurationMins: 300,
          usedPercent: String(100 - value),
          remainingPercent: String(value),
          remainingFraction: value / 100,
          resetsAt: 1_792_000_000,
        },
        weekly: {
          windowDurationMins: 10_080,
          usedPercent: String(100 - value),
          remainingPercent: String(value),
          remainingFraction: value / 100,
          resetsAt: 1_792_000_000,
        },
      }), remaining);

      for (const selector of ["#five-hour-progress", "#weekly-progress"]) {
        const meter = page.locator(selector);
        await expect(meter).toHaveAttribute("data-heat", heat);
        const mid = palette[heat][1];
        const [red, green, blue] = mid.match(/[\da-f]{2}/gi).map((channel) => Number.parseInt(channel, 16));
        const expectedTextColor = `rgb(${red}, ${green}, ${blue})`;
        await expect.poll(() => meter.evaluate((node) => {
          const percentage = node.closest(".quota-card").querySelector(".remaining-block strong");
          return getComputedStyle(percentage).color;
        })).toBe(expectedTextColor);
        const actual = await meter.evaluate((node) => {
          const style = getComputedStyle(node);
          const card = node.closest(".quota-card");
          const percentage = card.querySelector(".remaining-block strong");
          const track = node.querySelector(".meter-track").getBoundingClientRect();
          const fill = node.querySelector(".meter-fill").getBoundingClientRect();
          return {
            cardHeat: card.dataset.heat,
            colors: ["--water-deep", "--water-mid", "--water-light"]
              .map((property) => style.getPropertyValue(property).trim()),
            percentageColor: getComputedStyle(percentage).color,
            value: Number(node.getAttribute("aria-valuenow")),
            fillRatio: track.width === 0 ? 0 : fill.width / track.width * 100,
          };
        });
        expect(actual.cardHeat).toBe(heat);
        expect(actual.colors).toEqual(palette[heat]);
        expect(actual.percentageColor, JSON.stringify(actual)).toBe(expectedTextColor);
        expect(actual.value).toBeCloseTo(remaining, 2);
        expect(Math.abs(actual.fillRatio - remaining)).toBeLessThanOrEqual(0.1);
      }
    }

    await page.evaluate((state) => window.__mockEmitUsage(state), usageState(54, 38));
    await expect(page.locator("#five-hour-percent")).toHaveText("54%");
    await expect(page.locator("#weekly-percent")).toHaveText("38%");
    const expectedColors = ["rgb(203, 161, 66)", "#cba142", "rgb(220, 112, 64)", "#dc7040"];
    await expect.poll(() => page.evaluate(() => [
      getComputedStyle(document.querySelector("#five-hour-percent")).color,
      getComputedStyle(document.querySelector("#five-hour-progress")).getPropertyValue("--water-mid").trim(),
      getComputedStyle(document.querySelector("#weekly-percent")).color,
      getComputedStyle(document.querySelector("#weekly-progress")).getPropertyValue("--water-mid").trim(),
    ])).toEqual(expectedColors);
  }
});

test("keeps the divider between periods and removes the line under weekly reset", async ({ page }) => {
  await installTauriMock(page, {});
  await expect.poll(async () => page.locator("#five-hour-card").evaluate((card) => getComputedStyle(card).borderBottomWidth))
    .toBe("1px");
  await expect.poll(async () => page.locator("#weekly-card").evaluate((card) => getComputedStyle(card).borderBottomWidth))
    .toBe("0px");
});

test("renders quota events immediately, keeps source precision, and recovers from stale data", async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-10-14T15:00:00.000Z"));
  await installTauriMock(page, {});
  const updated = usageState(3, 82, "online");
  updated.fiveHour.remainingFraction = 0.03125;
  updated.fiveHour.remainingPercent = "3.125";
  updated.weekly.remainingFraction = 0.8275;
  updated.weekly.remainingPercent = "82.75";
  await page.evaluate((state) => window.__mockEmitUsage(state), updated);

  await expect(page.locator("#five-hour-percent")).toHaveText("3.125%");
  await expect(page.locator("#weekly-percent")).toHaveText("82.75%");
  await expect(page.locator("#five-hour-progress")).toHaveAttribute("aria-valuenow", "3.125");
  await expect(page.locator("#weekly-progress")).toHaveAttribute("aria-valuenow", "82.75");
  await expect(page.locator("#five-hour-card")).toHaveClass(/quota--critical/);
  await expect(page.locator("#weekly-card")).not.toHaveClass(/quota--warning/);
  await expect(page.locator("#connection-message")).toBeHidden();

  await page.evaluate((state) => window.__mockEmitUsage({ ...state, status: "reconnecting" }), updated);
  await expect(page.locator("#connection-message")).toHaveText("Reconnecting. Limits are temporarily unavailable.");
  await expectLimitsUnavailable(page, { width: 360, height: 230 });

  await page.evaluate((state) => window.__mockEmitUsage(state), usageState(18, 12, "online"));
  await expect(page.locator("#five-hour-percent")).toHaveText("18%");
  await expect(page.locator("#connection-message")).toBeHidden();
  await expect(page.locator("#five-hour-card")).not.toHaveClass(/quota--unavailable/);
  await expect(page.locator("#weekly-card")).not.toHaveClass(/quota--unavailable/);
});

test("marks missing and malformed windows unavailable without inventing percentages", async ({ page }) => {
  await installTauriMock(page, { usageState: usageState(51, null) });
  await expect(page.locator("#five-hour-percent")).toHaveText("51%");
  await expect(page.locator("#weekly-percent")).toHaveText("—");
  await expect(page.locator("#weekly-progress")).not.toHaveAttribute("aria-valuenow", /.+/);
  await expect(page.locator("#weekly-progress")).toHaveAttribute("aria-valuetext", "Usage limit unavailable.");
  await expect(page.locator("#weekly-reset")).toHaveText("Reset time unavailable");

  await page.evaluate(() => window.__mockEmitUsage({
    status: "online",
    fiveHour: {
      windowDurationMins: 300,
      remainingFraction: Number.NaN,
      remainingPercent: "not-a-number",
      resetsAt: 0,
    },
    weekly: null,
  }));
  await expect(page.locator("#five-hour-percent")).toHaveText("—");
  await expect(page.locator("#five-hour-progress")).toHaveAttribute("data-heat", "unavailable");
  await expect(page.locator("#five-hour-reset")).toHaveText("Reset time unavailable");
  await expect(page.locator("#weekly-progress")).toHaveAttribute("data-heat", "unavailable");
});

test("uses independent alert thresholds at warning and critical boundaries", async ({ page }) => {
  await installTauriMock(page, {
    settings: { threshold5h: 25, thresholdWeekly: 50 },
    usageState: usageState(25, 50),
  });
  await expect(page.locator("#five-hour-card")).toHaveClass(/quota--warning/);
  await expect(page.locator("#weekly-card")).toHaveClass(/quota--warning/);
  await expect(page.locator("#five-hour-card")).not.toHaveClass(/quota--critical/);
  await expect(page.locator("#weekly-card")).not.toHaveClass(/quota--critical/);

  await page.evaluate((state) => window.__mockEmitUsage(state), usageState(10, 10));
  await expect(page.locator("#five-hour-card")).toHaveClass(/quota--critical/);
  await expect(page.locator("#weekly-card")).toHaveClass(/quota--critical/);
  await page.evaluate((state) => window.__mockEmitUsage(state), usageState(26, 51));
  await expect(page.locator("#five-hour-card")).not.toHaveClass(/quota--warning/);
  await expect(page.locator("#weekly-card")).not.toHaveClass(/quota--warning/);
});

test("supports keyboard navigation and localized quota labels", async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-10-14T15:00:00.000Z"));
  await installTauriMock(page, {});
  await page.getByRole("button", { name: "Settings" }).click();
  const generalTab = page.getByRole("tab", { name: "General" });
  const limitsTab = page.getByRole("tab", { name: "Limits" });
  await generalTab.focus();
  await page.keyboard.press("ArrowRight");
  await expect(limitsTab).toHaveAttribute("aria-selected", "true");
  await expect(limitsTab).toBeFocused();
  await page.keyboard.press("Home");
  await expect(generalTab).toHaveAttribute("aria-selected", "true");
  await expect(generalTab).toBeFocused();

  await page.getByRole("button", { name: "Español" }).click();
  await page.getByRole("button", { name: "Volver" }).click();
  await expect(page.locator("#five-hour-progress")).toHaveAttribute(
    "aria-label",
    "Porcentaje restante en la ventana de 5 horas",
  );
  await expect(page.locator("#five-hour-reset")).toContainText("Se restablece");
  await expect(page.locator("#weekly-percent")).toHaveText("64 %");

  await page.getByRole("button", { name: "Configuración" }).click();
  await page.getByRole("button", { name: "English" }).click();
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  await expect(page.locator("#language-en")).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Back" }).click();
  await expect(page.locator("#five-hour-progress")).toHaveAttribute(
    "aria-label",
    "Percentage remaining in the 5-hour window",
  );
  await expect(page.locator("#weekly-percent")).toHaveText("64%");
});

test("toggles ultracompact mode, expands settings temporarily, and restores it", async ({ page }) => {
  await installTauriMock(page, {});
  const compactButton = page.getByRole("button", { name: "Turn on ultra-compact mode" });
  await compactButton.click();
  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "true");
  await expect.poll(async () => (await commandCalls(page, "set_ultra_compact")).length).toBe(1);
  await page.setViewportSize({ width: 300, height: 156 });
  await expect.poll(async () => page.locator("#five-hour-title").evaluate((node) => getComputedStyle(node).fontSize))
    .toBe("13px");
  await verifyVisibleLayout(page, { width: 300, height: 156 });
  await expect.poll(async () => page.evaluate(() => localStorage.getItem("codex-mystats-ui-settings")))
    .toContain('"ultraCompact":true');
  await page.reload();
  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "true");
  await page.setViewportSize({ width: 300, height: 156 });

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.locator("#settings-panel")).toBeVisible();
  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "false");
  expect(await commandCalls(page, "set_ultra_compact")).toEqual([
    { command: "set_ultra_compact", args: { ultraCompact: true } },
    { command: "set_ultra_compact", args: { ultraCompact: false } },
  ]);
  await page.setViewportSize({ width: 360, height: 230 });

  await page.getByRole("tab", { name: "Limits" }).click();
  await expect(page.locator("#limits-panel")).toBeVisible();
  await page.getByRole("tab", { name: "General" }).click();
  await page.getByRole("button", { name: "Español" }).click();
  await expect(page.locator("html")).toHaveAttribute("lang", "es-AR");
  await expect(page.locator("#settings-title")).toHaveText("Configuración");
  await expect.poll(async () => page.evaluate(() => localStorage.getItem("codex-mystats-ui-settings")))
    .toContain('"language":"es"');

  await page.getByRole("switch", { name: "Mantener la ventana siempre encima" }).click();
  const alwaysOnTopSwitch = page.getByRole("switch", { name: "Mantener la ventana siempre encima" });
  await expect(alwaysOnTopSwitch).not.toBeChecked();
  await expect.poll(async () => (await commandCalls(page, "set_always_on_top")).length).toBe(2);
  expect((await commandCalls(page, "set_always_on_top")).at(-1)).toEqual({
    command: "set_always_on_top",
    args: { alwaysOnTop: false },
  });
  await expect.poll(async () => page.evaluate(() => localStorage.getItem("codex-mystats-ui-settings")))
    .toContain('"alwaysOnTop":false');
  await page.getByRole("button", { name: "Volver" }).click();
  await expect(page.locator("#settings-panel")).toBeHidden();
  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "true");
  await page.setViewportSize({ width: 300, height: 156 });
  await verifyVisibleLayout(page, { width: 300, height: 156 });

  await page.getByRole("button", { name: "Minimizar" }).click();
  await expect.poll(async () => (await commandCalls(page, "minimize_panel")).length).toBe(1);
  await page.getByRole("button", { name: "Cerrar" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await verifyCloseDialogLayout(page);
  await expect(page.getByRole("button", { name: "Cancelar" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Minimizar en la bandeja" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Salir de Codex-MyStats" })).toBeVisible();
  await page.getByRole("button", { name: "Cancelar" }).click();
  await expect(page.getByRole("dialog")).toBeHidden();
  await page.getByRole("button", { name: "Cerrar" }).click();
  await page.getByRole("button", { name: "Salir de Codex-MyStats" }).click();
  await expect.poll(async () => (await commandCalls(page, "quit_app")).length).toBe(1);
});

test("keeps every compact close action visible together when saving the choice fails", async ({ page }) => {
  await installTauriMock(page, { settings: { ultraCompact: true }, failures: ["save_settings"] });
  await page.setViewportSize({ width: 300, height: 156 });
  await page.getByRole("button", { name: "Close" }).click();
  await page.getByRole("checkbox", { name: "Remember my choice" }).check();
  await page.getByRole("button", { name: "Minimize in the tray" }).click();

  await expect(page.locator("#close-preference-error"))
    .toHaveText("Could not save this choice. Please try again.");
  await expect(page.getByRole("button", { name: "Cancel" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Minimize in the tray" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Quit Codex-MyStats" })).toBeVisible();
  await verifyCloseDialogLayout(page);

  await fs.mkdir(previewDirectory, { recursive: true });
  await page.screenshot({
    path: path.join(previewDirectory, "close-dialog-ultracompact-error.png"),
    animations: "disabled",
    caret: "hide",
  });
});

test("remembers the close action across restart and can restore asking each time", async ({ page }) => {
  await installTauriMock(page, {});
  await page.getByRole("button", { name: "Close" }).click();
  await page.getByRole("checkbox", { name: "Remember my choice" }).check();
  await page.getByRole("button", { name: "Minimize in the tray" }).click();
  await expect.poll(async () => page.evaluate(() => localStorage.getItem("codex-mystats-ui-settings")))
    .toContain('"closeAction":"minimize"');
  await expect.poll(async () => (await commandCalls(page, "minimize_panel")).length).toBe(1);

  await page.reload();
  await page.getByRole("button", { name: "Close" }).click();
  await expect(page.getByRole("dialog")).toBeHidden();
  await expect.poll(async () => (await commandCalls(page, "minimize_panel")).length).toBe(1);

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.locator("#remembered-close-setting")).toBeVisible();
  await expect(page.locator("#remembered-close-text")).toHaveText("Minimize in the tray");
  await page.getByRole("button", { name: "Ask every time" }).click();
  await expect(page.locator("#remembered-close-setting")).toBeHidden();
  await expect.poll(async () => page.evaluate(() => localStorage.getItem("codex-mystats-ui-settings")))
    .toContain('"closeAction":"ask"');

  await page.reload();
  await page.getByRole("button", { name: "Close" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("does not minimize or quit when remembering a close choice fails to save", async ({ page }) => {
  await installTauriMock(page, {});
  await page.evaluate(() => window.__mockFailures.add("save_settings"));
  await page.getByRole("button", { name: "Close" }).click();
  await page.getByRole("checkbox", { name: "Remember my choice" }).check();
  await page.getByRole("button", { name: "Minimize in the tray" }).click();

  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.locator("#close-preference-error")).toHaveText("Could not save this choice. Please try again.");
  expect(await commandCalls(page, "minimize_panel")).toHaveLength(0);
  expect(await commandCalls(page, "quit_app")).toHaveLength(0);
});

test("reverts the always-on-top control and shows an error when the native change fails", async ({ page }) => {
  await installTauriMock(page, {});
  await page.evaluate(() => window.__mockFailures.add("set_always_on_top"));
  await page.getByRole("button", { name: "Settings" }).click();
  const alwaysOnTop = page.getByRole("switch", { name: "Keep the window on top" });
  await expect(alwaysOnTop).toBeChecked();
  await alwaysOnTop.click();
  await expect(alwaysOnTop).toBeChecked();
  await expect(page.locator("#general-panel .save-message")).toHaveText("Could not change always-on-top.");
});

test("hides cached percentages whenever limits are unavailable in both layouts", async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-10-14T15:00:00.000Z"));
  await installTauriMock(page, {
    usageState: usageState(61, 69, "online"),
  });

  const unavailableStatuses = [
    "connecting",
    "reconnecting",
    "stale",
    "disconnected",
    "needsLogin",
    "cliMissing",
    "cliCheckFailed",
    "incompatible",
  ];

  await expect(page.locator("#five-hour-percent")).toHaveText("61%");
  await expect(page.locator("#weekly-percent")).toHaveText("69%");
  await page.evaluate((state) => window.__mockEmitUsage(state), usageState(61, 69, "stale"));
  await expect(page.locator("#connection-message")).toHaveText("Could not update usage limits.");
  await expectLimitsUnavailable(page, { width: 360, height: 230 });
  for (const status of unavailableStatuses) {
    await page.evaluate((value) => window.__mockEmitUsage({
      ...window.__mockCurrentUsage,
      status: value,
    }), status);
    await expectLimitsUnavailable(page, { width: 360, height: 230 });
  }
  await page.evaluate(() => window.__mockEmitUsage({
    ...window.__mockCurrentUsage,
    status: "stale",
  }));
  await expectLimitsUnavailable(page, { width: 360, height: 230 });
  const unavailableColors = await page.locator("#five-hour-progress").evaluate((meter) => {
    const style = getComputedStyle(meter);
    return ["--water-deep", "--water-mid", "--water-light"]
      .map((property) => style.getPropertyValue(property).trim());
  });
  expect(unavailableColors).toEqual(["#4d535b", "#696b74", "#9696a0"]);

  await fs.mkdir(previewDirectory, { recursive: true });
  await page.locator(".app-shell").screenshot({
    path: path.join(previewDirectory, "unavailable-limits-normal.png"),
    animations: "disabled",
    caret: "hide",
  });

  await page.locator("#ultra-compact-button").click();
  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "true");
  await page.setViewportSize({ width: 300, height: 156 });
  await expectLimitsUnavailable(page, { width: 300, height: 156 });
  await page.locator(".app-shell").screenshot({
    path: path.join(previewDirectory, "unavailable-limits-ultracompact.png"),
    animations: "disabled",
    caret: "hide",
  });
  for (const status of unavailableStatuses) {
    await page.evaluate((value) => window.__mockEmitUsage({
      ...window.__mockCurrentUsage,
      status: value,
    }), status);
    await expectLimitsUnavailable(page, { width: 300, height: 156 });
  }
});

test("shows only the fresh window while a partial usage event is being reconciled", async ({ page }) => {
  await installTauriMock(page, {});
  await page.evaluate(() => window.__mockEmitUsage({
    status: "refreshing",
    fiveHour: {
      windowDurationMins: 300,
      usedPercent: "69",
      remainingPercent: "31",
      remainingFraction: 0.31,
      resetsAt: 1_792_000_000,
    },
    weekly: null,
  }));

  await expect(page.locator("#five-hour-percent")).toHaveText("31%");
  await expect(page.locator("#weekly-percent")).toHaveText("—");
  await expect(page.locator("#weekly-progress")).toHaveAttribute("data-heat", "unavailable");
  await expect(page.locator("#weekly-progress")).not.toHaveAttribute("aria-valuenow", /.+/);
  await expect(page.locator("#weekly-reset")).toHaveText("Reset time unavailable");
});

test("shows useful messages for sign-in, missing CLI, and incompatible CLI states", async ({ page }) => {
  await installTauriMock(page, {});
  const states = [
    ["needsLogin", "Sign in to Codex to view your limits."],
    ["cliMissing", "Codex CLI not found. Install it and restart the app."],
    ["cliCheckFailed", "Could not check Codex CLI. Restart the app."],
    ["incompatible", "Update Codex CLI to a compatible version."],
  ];
  for (const [status, message] of states) {
    await page.evaluate((value) => window.__mockEmitUsage({
      status: value,
      fiveHour: null,
      weekly: null,
    }), status);
    await expect(page.locator("#connection-message")).toHaveText(message);
    await expect(page.locator("#five-hour-percent")).toHaveText("—");
    await expect(page.locator("#weekly-percent")).toHaveText("—");
  }
});

test("keeps the build version visible when startup cannot read usage", async ({ page }) => {
  await installTauriMock(page, { usageFailure: true });
  await expect(page.locator("#connection-message")).toHaveText("Could not retrieve usage limits.");
  await expect(page.locator("#app-version-badge")).toHaveText(`v${appVersion}`);
  await expect(page.locator("#five-hour-percent")).toHaveText("—");
  await expect(page.locator("#weekly-percent")).toHaveText("—");
});

test("hides an early cached usage event when the startup read fails", async ({ page }) => {
  await installTauriMock(page, {
    usageFailure: true,
    usageEventOnListen: usageState(61, 69, "online"),
  });

  await expect(page.locator("#connection-message")).toHaveText("Could not retrieve usage limits.");
  await expectLimitsUnavailable(page, { width: 360, height: 230 });
});

test("shows a disconnected state when event subscription fails during startup", async ({ page }) => {
  await installTauriMock(page, { listenFailure: true });
  await expect(page.locator("#connection-message")).toHaveText("Could not retrieve usage limits.");
  await expect(page.locator("#app-version-badge")).toHaveText(`v${appVersion}`);
  await expect(page.locator("#five-hour-percent")).toHaveText("—");
  await expect(page.locator("#weekly-percent")).toHaveText("—");
  await expect(page.locator("#five-hour-card")).toHaveClass(/quota--unavailable/);
  await expect(page.locator("#weekly-card")).toHaveClass(/quota--unavailable/);
});

test("keeps the version badge visible if version lookup fails", async ({ page }) => {
  await installTauriMock(page, { versionFailure: true });
  const badge = page.locator("#app-version-badge");
  await expect(badge).toHaveText("v—");
  await expect(badge).toBeVisible();
  await expect(badge).toHaveAttribute("aria-label", "Version unavailable");
});

test("reports settings load and save failures", async ({ page }) => {
  await installTauriMock(page, { loadFailure: true, saveFailure: true });
  await expect(page.locator("#connection-message")).toBeVisible();
  await expect(page.locator("#app-version-badge")).toHaveText(`v${appVersion}`);

  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("button", { name: "Español" }).click();
  await expect(page.locator("#general-panel .save-message")).toHaveText("No se pudieron guardar los cambios.");
});

test("reports failure to resize without persisting compact mode", async ({ page }) => {
  await installTauriMock(page, { failures: ["set_ultra_compact"] });
  await page.getByRole("button", { name: "Turn on ultra-compact mode" }).click();
  await expect(page.locator("#connection-message")).toHaveText("Could not change the window size.");
  await expect(page.locator(".app-shell")).toHaveAttribute("data-ultra-compact", "false");
  await expect(page.locator("#ultra-compact-button")).toHaveAttribute("aria-pressed", "false");
});
