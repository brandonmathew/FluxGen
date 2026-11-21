"use strict";

/* FluxGen front-end. Vanilla JS, no build step.
   All data comes from Tauri commands; every failure is shown in
   the error banner with its full reason. */

const invoke = window.__TAURI__.core.invoke;

const state = {
  variants: [],
  queue: [],
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

/// Format a byte count in the largest unit that keeps the
/// value at or above 1, so small files read as MB or KB
/// instead of a fraction of a GB.
function formatBytes(bytes) {
  if (bytes === null || bytes === undefined) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  const decimals = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return value.toFixed(decimals) + " " + units[unit];
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
    info.vram_bytes === null ? "Not detected" : formatBytes(info.vram_bytes);
  document.getElementById("sys-ram").textContent = formatBytes(info.ram_bytes);
  document.getElementById("sys-disk").textContent = formatBytes(info.free_disk_bytes) + " free";
  document.getElementById("sys-data").textContent = info.data_root;
}

// ---------------------------------------------------------------------------
// Variants and models
// ---------------------------------------------------------------------------

async function loadEngineInfo() {
  const info = await call("get_engine_info");
  if (!info) {
    return;
  }
  const line = document.getElementById("engine-info");
  const controls = document.getElementById("engine-download-controls");
  const button = document.getElementById("download-button-engine");
  button.dataset.idleLabel =
    info.state === "outdated" ? "Update engine" : "Download engine";
  if (!activeDownloads.has("engine")) {
    button.textContent = button.dataset.idleLabel;
  }
  if (info.state === "current") {
    line.textContent = "Engine: " + info.version + " — installed.";
    controls.hidden = true;
  } else if (info.state === "outdated") {
    line.textContent =
      "Engine: an older engine is installed. The pinned version is " +
      info.version +
      ".";
    controls.hidden = false;
  } else {
    line.textContent =
      "Engine: not installed. " +
      info.version +
      " downloads when you ask (about 1 GB).";
    controls.hidden = false;
  }
}

async function loadVariants() {
  await loadEngineInfo();
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
    row.appendChild(el("td", null, formatBytes(quant.size_bytes)));
    row.appendChild(el("td", null, formatBytes(quant.total_size_bytes)));
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
        formatBytes(choice.model_size_bytes) +
        ")\n" +
        choice.reasons.join("\n")
    );
    card.appendChild(rec);
  }

  const controls = el("div", "download-controls");
  const quantSelect = el("select", "download-quant");
  quantSelect.id = "download-quant-" + variant.id;
  for (const quant of variant.quants) {
    const option = document.createElement("option");
    option.value = quant.label;
    option.textContent =
      quant.label + " — " + formatBytes(quant.total_size_bytes) + " total";
    quantSelect.appendChild(option);
  }
  if (choice) {
    quantSelect.value = choice.label;
  }
  controls.appendChild(quantSelect);

  const button = el(
    "button",
    "download-button",
    variant.installed ? "Download again" : "Download"
  );
  button.type = "button";
  button.id = "download-button-" + variant.id;
  button.dataset.idleLabel = button.textContent;
  button.addEventListener("click", () =>
    toggleDownload(variant, quantSelect, button)
  );
  controls.appendChild(button);
  card.appendChild(controls);

  const progressWrap = el("div", "download-progress");
  progressWrap.id = "download-progress-" + variant.id;
  progressWrap.hidden = true;
  const track = el("div", "progress-track");
  const fill = el("div", "progress-fill");
  fill.id = "download-fill-" + variant.id;
  track.appendChild(fill);
  progressWrap.appendChild(track);
  const status = el("p", "download-status");
  status.id = "download-status-" + variant.id;
  progressWrap.appendChild(status);
  card.appendChild(progressWrap);

  return card;
}

// ---------------------------------------------------------------------------
// Variant downloads
// ---------------------------------------------------------------------------

const activeDownloads = new Set();

function setDownloadUi(variantId, running) {
  const button = document.getElementById("download-button-" + variantId);
  const select = document.getElementById("download-quant-" + variantId);
  const wrap = document.getElementById("download-progress-" + variantId);
  if (button) {
    button.textContent = running
      ? "Cancel download"
      : button.dataset.idleLabel || "Download";
  }
  if (select) {
    select.disabled = running;
  }
  if (wrap) {
    wrap.hidden = !running;
  }
}

async function toggleDownload(variant, quantSelect) {
  if (activeDownloads.has(variant.id)) {
    await call("cancel_variant_download", { variantId: variant.id });
    return;
  }
  const started = await call("start_variant_download", {
    variantId: variant.id,
    quantLabel: quantSelect.value,
  });
  if (started === undefined) {
    return;
  }
  activeDownloads.add(variant.id);
  setDownloadUi(variant.id, true);
}

window.__TAURI__.event.listen("download-progress", (event) => {
  const p = event.payload;
  const fill = document.getElementById("download-fill-" + p.variant_id);
  const status = document.getElementById("download-status-" + p.variant_id);
  if (!fill || !status) {
    return;
  }
  const percent =
    p.overall_total_bytes > 0
      ? Math.floor((p.overall_downloaded_bytes * 100) / p.overall_total_bytes)
      : 0;
  fill.style.width = percent + "%";
  status.textContent =
    "File " +
    p.file_index +
    " of " +
    p.files_total +
    " — " +
    p.file_name +
    " — " +
    formatBytes(p.overall_downloaded_bytes) +
    " of " +
    formatBytes(p.overall_total_bytes) +
    " (" +
    percent +
    "%)";
});

window.__TAURI__.event.listen("download-finished", async (event) => {
  const f = event.payload;
  activeDownloads.delete(f.variant_id);
  if (f.variant_id === "engine") {
    setDownloadUi("engine", false);
    await loadEngineInfo();
    if (!f.ok && f.error) {
      showError(f.error, "start_engine_download");
    }
    return;
  }
  if (f.ok) {
    await loadVariants();
    for (const id of activeDownloads) {
      setDownloadUi(id, true);
    }
  } else {
    setDownloadUi(f.variant_id, false);
    if (f.error) {
      showError(f.error, "start_variant_download");
    }
  }
});

document
  .getElementById("download-button-engine")
  .addEventListener("click", async () => {
    if (activeDownloads.has("engine")) {
      await call("cancel_engine_download", {});
      return;
    }
    const started = await call("start_engine_download", {});
    if (started === undefined) {
      return;
    }
    activeDownloads.add("engine");
    setDownloadUi("engine", true);
  });

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

function jobStatusText(job) {
  switch (job.status.state) {
    case "queued":
      return "Queued";
    case "running":
      return "Running…";
    case "completed":
      return "Done";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Cancelled";
    default:
      return job.status.state;
  }
}

function renderQueue() {
  const list = document.getElementById("queue-list");
  list.textContent = "";
  if (state.queue.length === 0) {
    list.appendChild(el("li", "queue-empty", "Queue is empty."));
    return;
  }
  for (const job of state.queue) {
    const item = el("li", "queue-item");
    const variant = state.variants.find((v) => v.id === job.variant);
    const variantName = variant ? variant.display_name : job.variant;
    const prompt = job.settings.prompt;
    const preview = prompt.length > 48 ? prompt.slice(0, 48) + "…" : prompt;
    const label =
      "#" + job.id + " · " + variantName + " · " + job.settings.width + "×" +
      job.settings.height + " · " + job.settings.steps + " steps · “" + preview + "”";
    const main = el("div", "q-main");
    main.appendChild(el("span", null, label));
    main.appendChild(el("span", "q-status", jobStatusText(job)));
    item.appendChild(main);
    if (job.status.state === "failed") {
      item.appendChild(el("div", "q-reason", job.status.reason));
    }
    const actions = el("div", "q-actions");
    if (job.status.state === "queued" || job.status.state === "running") {
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.textContent = "Cancel";
      cancel.addEventListener("click", async () => {
        await call("cancel_job", { jobId: job.id });
      });
      actions.appendChild(cancel);
    }
    if (job.status.state === "completed") {
      const view = document.createElement("button");
      view.type = "button";
      view.textContent = "View";
      view.addEventListener("click", () => showJobOutput(job, 0));
      actions.appendChild(view);
    }
    if (
      job.status.state === "completed" ||
      job.status.state === "failed" ||
      job.status.state === "cancelled"
    ) {
      const dismiss = document.createElement("button");
      dismiss.type = "button";
      dismiss.textContent = "Dismiss";
      dismiss.addEventListener("click", async () => {
        const result = await call("dismiss_job", { jobId: job.id });
        if (result !== undefined) {
          state.queue = state.queue.filter((j) => j.id !== job.id);
          renderQueue();
        }
      });
      actions.appendChild(dismiss);
    }
    item.appendChild(actions);
    list.appendChild(item);
  }
}

async function showJobOutput(job, index) {
  const url = await call("get_job_output", { jobId: job.id, index });
  if (!url) return;
  const stage = document.getElementById("preview-stage");
  stage.textContent = "";
  const img = document.createElement("img");
  img.src = url;
  img.alt = job.settings.prompt;
  stage.appendChild(img);
  const meta = document.getElementById("preview-meta");
  meta.textContent =
    job.settings.prompt +
    " · seed " + (job.settings.seed ?? "random") +
    " · " + job.settings.width + "×" + job.settings.height +
    " · " + job.settings.steps + " steps";
}

document.getElementById("gen-enqueue").addEventListener("click", async () => {
  clearError();
  const prompt = document.getElementById("gen-prompt").value.trim();
  if (!prompt) {
    showError("enqueue_generation: Invalid value for prompt: prompt must not be empty");
    return;
  }
  const variantId = document.getElementById("gen-variant").value;
  if (!variantId) {
    showError("enqueue_generation: No variant selected. Download a variant on the Models page first.");
    return;
  }
  const seedText = document.getElementById("gen-seed").value.trim();
  const initImage = document.getElementById("gen-init-image").value.trim();
  const job = await call("enqueue_generation", {
    request: {
      variantId,
      prompt,
      negativePrompt: document.getElementById("gen-negative").value,
      seed: seedText === "" ? null : Number(seedText),
      steps: Number(document.getElementById("gen-steps").value) || 20,
      guidance: Number(document.getElementById("gen-guidance").value) || 3.5,
      sampler: document.getElementById("gen-sampler").value,
      width: Number(document.getElementById("gen-width").value) || 1024,
      height: Number(document.getElementById("gen-height").value) || 1024,
      initImage: initImage === "" ? null : initImage,
      denoiseStrength:
        initImage === ""
          ? null
          : Number(document.getElementById("gen-strength").value) || 0.75,
    },
  });
  if (!job) return;
  state.queue.push(job);
  renderQueue();
});

window.__TAURI__.event.listen("job-updated", async (event) => {
  const updated = event.payload;
  const at = state.queue.findIndex((j) => j.id === updated.id);
  if (at >= 0) state.queue[at] = updated;
  else state.queue.push(updated);
  renderQueue();
  if (updated.status.state === "completed") {
    showJobOutput(updated, 0);
    loadGallery();
  }
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
    const item = el("li", "lora-item");
    item.appendChild(el("span", "lora-name", lora.name));

    const enabledBox = document.createElement("input");
    enabledBox.type = "checkbox";
    enabledBox.checked = lora.enabled;
    enabledBox.title = "Enabled";
    const weightInput = document.createElement("input");
    weightInput.type = "number";
    weightInput.min = "0";
    weightInput.max = "2";
    weightInput.step = "0.05";
    weightInput.value = String(lora.weight);
    weightInput.title = "Weight";
    const applyChange = async () => {
      const result = await call("update_lora", {
        variantId,
        name: lora.name,
        weight: Number(weightInput.value) || 0,
        enabled: enabledBox.checked,
      });
      if (result !== undefined) loadLoras();
    };
    enabledBox.addEventListener("change", applyChange);
    weightInput.addEventListener("change", applyChange);

    const controls = el("span", "lora-controls");
    controls.appendChild(enabledBox);
    controls.appendChild(document.createTextNode(" weight "));
    controls.appendChild(weightInput);
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Delete";
    remove.addEventListener("click", async () => {
      const result = await call("delete_lora", {
        variantId,
        name: lora.name,
      });
      if (result !== undefined) loadLoras();
    });
    controls.appendChild(remove);
    item.appendChild(controls);
    list.appendChild(item);
  }
}

document.getElementById("lora-variant").addEventListener("change", loadLoras);

document.getElementById("lora-import").addEventListener("click", async () => {
  clearError();
  const variantId = document.getElementById("lora-variant").value;
  if (!variantId) {
    showError("import_lora: Choose a variant first.");
    return;
  }
  const picked = await window.__TAURI__.dialog.open({
    multiple: false,
    directory: false,
    filters: [
      { name: "LoRA files", extensions: ["safetensors", "gguf"] },
    ],
  });
  if (typeof picked !== "string") return;
  const entry = await call("import_lora", { variantId, sourcePath: picked });
  if (entry) loadLoras();
});

// ---------------------------------------------------------------------------
// Image-to-image source picker
// ---------------------------------------------------------------------------

document
  .getElementById("gen-init-pick")
  .addEventListener("click", async () => {
    const picked = await window.__TAURI__.dialog.open({
      multiple: false,
      directory: false,
      filters: [
        { name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] },
      ],
    });
    if (typeof picked === "string") {
      document.getElementById("gen-init-image").value = picked;
    }
  });

document.getElementById("gen-init-clear").addEventListener("click", () => {
  document.getElementById("gen-init-image").value = "";
});

// ---------------------------------------------------------------------------
// Application updates
// ---------------------------------------------------------------------------

const updateStatus = document.getElementById("update-status");
const updateInstallButton = document.getElementById("update-install");
const updateProgress = document.getElementById("update-progress");

function showUpdateBanner(version) {
  document.getElementById("update-banner-text").textContent =
    "FluxGen " + version + " is available.";
  document.getElementById("update-banner").hidden = false;
}

/* Check for an update. In quiet mode (startup) a failed check
   stays silent and only a found update is surfaced. */
async function checkUpdates(quiet) {
  let info;
  try {
    info = await invoke("check_for_update");
  } catch (e) {
    if (!quiet) {
      showError(e, "check_for_update");
    }
    return;
  }
  if (info) {
    updateStatus.textContent = "Update available: FluxGen " + info.version + ".";
    updateInstallButton.hidden = false;
    updateInstallButton.textContent = "Download and install " + info.version;
    showUpdateBanner(info.version);
  } else if (!quiet) {
    updateStatus.textContent = "FluxGen is up to date.";
    updateInstallButton.hidden = true;
  }
}

document
  .getElementById("update-check")
  .addEventListener("click", () => checkUpdates(false));

document
  .getElementById("update-banner-view")
  .addEventListener("click", () => {
    document.getElementById("update-banner").hidden = true;
    document.querySelector('[data-page="settings"]').click();
  });

document
  .getElementById("update-banner-dismiss")
  .addEventListener("click", () => {
    document.getElementById("update-banner").hidden = true;
  });

updateInstallButton.addEventListener("click", async () => {
  updateInstallButton.disabled = true;
  updateProgress.textContent = "Downloading the update…";
  const result = await call("install_update", {});
  if (result === undefined) {
    updateInstallButton.disabled = false;
    updateProgress.textContent = "";
  }
  // On success the application exits as the installer launches.
});

window.__TAURI__.event.listen("update-progress", (event) => {
  updateProgress.textContent =
    "Downloading the update — " +
    formatBytes(event.payload.downloaded_bytes) +
    " downloaded.";
});

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

(async function init() {
  await checkCrashReport();
  await loadSystemInfo();
  await loadVariants();
  const jobs = await call("list_jobs");
  if (jobs) {
    state.queue = jobs;
    renderQueue();
  }
  await refreshEngineStatus();
  renderQueue();
  checkUpdates(true);
})();
