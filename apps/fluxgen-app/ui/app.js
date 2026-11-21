"use strict";

/* FluxGen front-end. Vanilla JS, no build step.
   All data comes from Tauri commands; every failure is shown in
   the error banner with its full reason. */

const invoke = window.__TAURI__.core.invoke;

const state = {
  variants: [],
  queue: [],
  nextJobId: 1,
  crashReport: null,
  errors: [],
};

// ---------------------------------------------------------------------------
// Error banner
// ---------------------------------------------------------------------------

const banner = document.getElementById("error-banner");
const bannerText = document.getElementById("error-banner-text");

function showError(message, source) {
  let text =
    message === null || message === undefined ? "" : String(message).trim();
  if (!text) text = "Unknown error (no message was provided)";
  const entry = {
    at: new Date(),
    source: source || "interface",
    text,
  };
  state.errors.push(entry);
  renderSessionErrors();
  bannerText.textContent = entry.source + ": " + entry.text;
  banner.hidden = false;
}

function renderSessionErrors() {
  const list = document.getElementById("session-errors");
  const empty = document.getElementById("session-errors-empty");
  if (!list) return;
  list.textContent = "";
  if (empty) empty.hidden = state.errors.length > 0;
  for (const entry of state.errors) {
    list.appendChild(
      el(
        "li",
        null,
        entry.at.toLocaleTimeString() + " · " + entry.source + ": " + entry.text
      )
    );
  }
}

function clearError() {
  banner.hidden = true;
  bannerText.textContent = "";
}

document.getElementById("error-dismiss").addEventListener("click", clearError);
document.getElementById("error-copy").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(bannerText.textContent);
  } catch (e) {
    showError("Copy failed: " + e, "clipboard");
  }
});

/* Run a command and surface any failure in the banner. Returns the
   result, or undefined on failure. */
async function call(command, args) {
  try {
    return await invoke(command, args);
  } catch (e) {
    showError(e, command);
    return undefined;
  }
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

const themeSelect = document.getElementById("theme-select");

function applyTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme);
  themeSelect.value = theme;
  try {
    localStorage.setItem("fluxgen-theme", theme);
  } catch (e) {
    /* storage unavailable: theme applies to this session only */
  }
}

themeSelect.addEventListener("change", () => applyTheme(themeSelect.value));

let savedTheme = "dark";
try {
  savedTheme = localStorage.getItem("fluxgen-theme") || "dark";
} catch (e) {
  savedTheme = "dark";
}
applyTheme(savedTheme === "light" ? "light" : "dark");

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

const nav = document.getElementById("nav");
nav.addEventListener("click", (event) => {
  const button = event.target.closest("[data-page]");
  if (!button) return;
  const page = button.dataset.page;
  nav.querySelectorAll(".nav-item").forEach((item) => {
    item.classList.toggle("active", item === button);
  });
  document.querySelectorAll(".page").forEach((section) => {
    section.classList.toggle("active", section.id === "page-" + page);
  });
  if (page === "gallery") loadGallery();
  if (page === "presets") loadPresets();
  if (page === "loras") loadLoras();
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatGB(bytes) {
  if (bytes === null || bytes === undefined) return "—";
  return (bytes / 1_000_000_000).toFixed(1) + " GB";
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

// ---------------------------------------------------------------------------
// Crash screen
// ---------------------------------------------------------------------------

async function checkCrashReport() {
  const report = await call("check_crash_report");
  if (!report) return;
  state.crashReport = report;
  document.getElementById("crash-file-name").textContent = report.file_name;
  document.getElementById("crash-contents").textContent = report.contents;
  document.getElementById("crash-screen").hidden = false;
}

document.getElementById("crash-copy").addEventListener("click", async () => {
  if (!state.crashReport) return;
  try {
    await navigator.clipboard.writeText(state.crashReport.contents);
  } catch (e) {
    showError("Copy failed: " + e, "clipboard");
  }
});

document.getElementById("crash-delete").addEventListener("click", async () => {
  if (!state.crashReport) return;
  const result = await call("delete_crash_report", {
    fileName: state.crashReport.file_name,
  });
  if (result !== undefined) {
    document.getElementById("crash-screen").hidden = true;
    state.crashReport = null;
  }
});

document.getElementById("crash-continue").addEventListener("click", () => {
  document.getElementById("crash-screen").hidden = true;
});

// ---------------------------------------------------------------------------
// System info
// ---------------------------------------------------------------------------

async function loadSystemInfo() {
  try {
    document.getElementById("sys-version").textContent =
      await window.__TAURI__.app.getVersion();
  } catch (e) {
    document.getElementById("sys-version").textContent = "—";
  }
  const info = await call("get_system_info");
  if (!info) return;
  document.getElementById("sys-gpu").textContent = info.gpu_name || "Not detected";
  document.getElementById("sys-vram").textContent =
    info.vram_bytes === null ? "Not detected" : formatGB(info.vram_bytes);
  document.getElementById("sys-ram").textContent = formatGB(info.ram_bytes);
  document.getElementById("sys-disk").textContent = formatGB(info.free_disk_bytes) + " free";
  document.getElementById("sys-data").textContent = info.data_root;
}

// ---------------------------------------------------------------------------
// Variants and models
// ---------------------------------------------------------------------------

async function loadVariants() {
  const engineInfo = await call("get_engine_info");
  if (engineInfo) {
    document.getElementById("engine-info").textContent = engineInfo.bundled
      ? "Engine: " + engineInfo.version + " (bundled)"
      : "Engine: " + engineInfo.version + " — binary not found at " + engineInfo.binary_path;
  }
  const variants = await call("list_variants");
  if (!variants) return;
  state.variants = variants;

  const genSelect = document.getElementById("gen-variant");
  const loraSelect = document.getElementById("lora-variant");
  genSelect.textContent = "";
  loraSelect.textContent = "";
  for (const variant of variants) {
    const option = document.createElement("option");
    option.value = variant.id;
    option.textContent = variant.installed
      ? variant.display_name
      : variant.display_name + " (not downloaded)";
    option.disabled = !variant.installed;
    genSelect.appendChild(option);

    const loraOption = document.createElement("option");
    loraOption.value = variant.id;
    loraOption.textContent = variant.display_name;
    loraSelect.appendChild(loraOption);
  }

  const list = document.getElementById("variant-list");
  list.textContent = "";
  for (const variant of variants) {
    list.appendChild(await variantCard(variant));
  }
}

async function variantCard(variant) {
  const card = el("article", "variant-card");
  const header = el("header");
  header.appendChild(el("h2", null, variant.display_name));
  const badge = el(
    "span",
    "badge" + (variant.installed ? " installed" : ""),
    variant.installed ? "Downloaded" : "Not downloaded"
  );
  header.appendChild(badge);
  card.appendChild(header);

  const table = el("table", "quant-table");
  const thead = el("thead");
  const headRow = el("tr");
  for (const heading of ["Quantization", "Model file", "Total download"]) {
    headRow.appendChild(el("th", null, heading));
  }
  thead.appendChild(headRow);
  table.appendChild(thead);
  const tbody = el("tbody");
  for (const quant of variant.quants) {
    const row = el("tr");
    row.appendChild(el("td", null, quant.label));
    row.appendChild(el("td", null, formatGB(quant.size_bytes)));
    row.appendChild(el("td", null, formatGB(quant.total_size_bytes)));
    tbody.appendChild(row);
  }
  table.appendChild(tbody);
  card.appendChild(table);

  const width = Number(document.getElementById("gen-width").value) || 1024;
  const height = Number(document.getElementById("gen-height").value) || 1024;
  const choice = await call("get_quant_recommendation", {
    variantId: variant.id,
    width,
    height,
  });
  if (choice) {
    const rec = el(
      "p",
      "recommendation",
      "Recommended: " +
        choice.label +
        " (" +
        formatGB(choice.model_size_bytes) +
        ")\n" +
        choice.reasons.join("\n")
    );
    card.appendChild(rec);
  }
  return card;
}

// ---------------------------------------------------------------------------
// Engine controls
// ---------------------------------------------------------------------------

const enginePill = document.getElementById("engine-pill");

async function refreshEngineStatus() {
  const status = await call("engine_status");
  if (!status) return;
  if (status.running && status.alive) {
    enginePill.textContent = "Engine: running on port " + status.port;
    enginePill.classList.add("running");
  } else if (status.running && !status.alive) {
    enginePill.textContent = "Engine: exited";
    enginePill.classList.remove("running");
    showError(
      "The engine process has exited. Its output is recorded on the failed job; restart the engine to continue."
    );
  } else {
    enginePill.textContent = "Engine: stopped";
    enginePill.classList.remove("running");
  }
}

document.getElementById("engine-start").addEventListener("click", async () => {
  clearError();
  const variantId = document.getElementById("gen-variant").value;
  const variant = state.variants.find((v) => v.id === variantId);
  if (!variant) {
    showError("No variant selected.");
    return;
  }
  const width = Number(document.getElementById("gen-width").value) || 1024;
  const height = Number(document.getElementById("gen-height").value) || 1024;
  const choice = await call("get_quant_recommendation", {
    variantId,
    width,
    height,
  });
  if (!choice) return;
  const result = await call("engine_start", {
    variantId,
    quantLabel: choice.label,
  });
  if (result !== undefined) {
    await refreshEngineStatus();
  }
});

document.getElementById("engine-stop").addEventListener("click", async () => {
  clearError();
  const result = await call("engine_stop");
  if (result !== undefined) {
    await refreshEngineStatus();
  }
});

// ---------------------------------------------------------------------------
// Queue (front-end job list for this stage)
// ---------------------------------------------------------------------------

function renderQueue() {
  const list = document.getElementById("queue-list");
  list.textContent = "";
  if (state.queue.length === 0) {
    list.appendChild(el("li", "queue-empty", "Queue is empty."));
    return;
  }
  for (const job of state.queue) {
    const item = el("li");
    const label =
      "#" + job.id + " · " + job.variantName + " · " + job.width + "×" + job.height +
      " · " + job.steps + " steps · “" + job.promptPreview + "”";
    item.appendChild(el("span", null, label));
    const status = el("span", "q-status", job.status);
    item.appendChild(status);
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = "Cancel";
    cancel.addEventListener("click", () => {
      job.status = "Cancelled";
      renderQueue();
    });
    item.appendChild(cancel);
    list.appendChild(item);
  }
}

document.getElementById("gen-enqueue").addEventListener("click", () => {
  clearError();
  const prompt = document.getElementById("gen-prompt").value.trim();
  if (!prompt) {
    showError("Invalid value for prompt: prompt must not be empty");
    return;
  }
  const variantId = document.getElementById("gen-variant").value;
  const variant = state.variants.find((v) => v.id === variantId);
  if (!variant) {
    showError("No variant selected. Download a variant on the Models page first.");
    return;
  }
  state.queue.push({
    id: state.nextJobId++,
    variantId,
    variantName: variant.display_name,
    promptPreview: prompt.length > 48 ? prompt.slice(0, 48) + "…" : prompt,
    width: Number(document.getElementById("gen-width").value) || 1024,
    height: Number(document.getElementById("gen-height").value) || 1024,
    steps: Number(document.getElementById("gen-steps").value) || 20,
    status: "Queued",
  });
  renderQueue();
});

// ---------------------------------------------------------------------------
// Gallery, presets, LoRAs
// ---------------------------------------------------------------------------

async function loadGallery() {
  const records = await call("list_gallery");
  const grid = document.getElementById("gallery-grid");
  const empty = document.getElementById("gallery-empty");
  grid.textContent = "";
  if (!records) return;
  empty.hidden = records.length > 0;
  for (const record of records) {
    const card = el("div", "gallery-card");
    card.appendChild(el("div", "g-path", record.path));
    let promptText = "";
    try {
      const settings = JSON.parse(record.settings_json);
      if (settings && settings.prompt) promptText = "\n" + settings.prompt;
    } catch (e) {
      promptText = "";
    }
    card.appendChild(
      el("div", "g-meta", record.variant + " · " + record.quant + promptText)
    );
    grid.appendChild(card);
  }
}

async function loadPresets() {
  const presets = await call("list_presets");
  const list = document.getElementById("preset-list");
  const empty = document.getElementById("preset-empty");
  list.textContent = "";
  if (!presets) return;
  empty.hidden = presets.length > 0;
  for (const preset of presets) {
    list.appendChild(
      el("li", null, preset.name + " · " + preset.loras.length + " LoRA(s)")
    );
  }
}

async function loadLoras() {
  const variantId = document.getElementById("lora-variant").value;
  if (!variantId) return;
  const loras = await call("list_loras", { variantId });
  const list = document.getElementById("lora-list");
  const empty = document.getElementById("lora-empty");
  list.textContent = "";
  if (!loras) return;
  empty.hidden = loras.length > 0;
  for (const lora of loras) {
    list.appendChild(
      el(
        "li",
        null,
        lora.name +
          " · weight " +
          lora.weight.toFixed(2) +
          (lora.enabled ? " · enabled" : " · disabled")
      )
    );
  }
}

document.getElementById("lora-variant").addEventListener("change", loadLoras);

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

(async function init() {
  await checkCrashReport();
  await loadSystemInfo();
  await loadVariants();
  await refreshEngineStatus();
  renderQueue();
})();
