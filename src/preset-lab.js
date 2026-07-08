import { createPreset } from "./presets.js";
import { RANDOM_FAMILIES, RANDOM_MODES, randomizePreset } from "./randomize.js";
import { clone, downloadBlob, fitWithin, safeFilename } from "./utils.js";

const DEFAULT_BATCH_SIZE = 100;
const DEFAULT_RENDER_SIZE = 300;
const COLLAPSED_THUMB_COUNT = 4;
const PRESET_TAG = "preset-lab";
const MAX_WORKERS = 4;

const dom = {
  folderInput: document.querySelector("#folderInput"),
  pickFolderButton: document.querySelector("#pickFolderButton"),
  generateBatchButton: document.querySelector("#generateBatchButton"),
  exportKeptButton: document.querySelector("#exportKeptButton"),
  copyKeptButton: document.querySelector("#copyKeptButton"),
  clearKeptButton: document.querySelector("#clearKeptButton"),
  batchSizeInput: document.querySelector("#batchSizeInput"),
  renderSizeInput: document.querySelector("#renderSizeInput"),
  randomMode: document.querySelector("#labRandomMode"),
  familyList: document.querySelector("#labFamilyList"),
  imageStatus: document.querySelector("#labImageStatus"),
  batchStatus: document.querySelector("#labBatchStatus"),
  presetCount: document.querySelector("#labPresetCount"),
  renderCount: document.querySelector("#labRenderCount"),
  keptCount: document.querySelector("#labKeptCount"),
  statusText: document.querySelector("#labStatusText"),
  emptyState: document.querySelector("#labEmptyState"),
  grid: document.querySelector("#labGrid")
};

const state = {
  images: [],
  presets: [],
  renderToken: 0,
  workers: [],
  pendingJobs: [],
  completedRenders: 0,
  totalRenders: 0
};

function init() {
  renderModes();
  renderFamilies();
  bindEvents();
  refreshStats();
}

function renderModes() {
  dom.randomMode.innerHTML = "";
  RANDOM_MODES.forEach(([value, label, description]) => {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label;
    option.title = description || "";
    if (value === "damaged") option.selected = true;
    dom.randomMode.append(option);
  });
}

function renderFamilies() {
  dom.familyList.innerHTML = "";
  RANDOM_FAMILIES.forEach(([value, label, description]) => {
    const item = document.createElement("label");
    item.className = "lab-family-option";
    item.title = description || "";
    item.innerHTML = `
      <input type="checkbox" value="${value}" checked>
      <span>${label}</span>
    `;
    dom.familyList.append(item);
  });
}

function bindEvents() {
  dom.pickFolderButton.addEventListener("click", () => dom.folderInput.click());
  dom.folderInput.addEventListener("change", () => {
    loadFolderImages([...dom.folderInput.files]);
    dom.folderInput.value = "";
  });
  dom.generateBatchButton.addEventListener("click", generateBatch);
  dom.exportKeptButton.addEventListener("click", exportKeptModule);
  dom.copyKeptButton.addEventListener("click", copyKeptEntries);
  dom.clearKeptButton.addEventListener("click", clearKept);
}

async function loadFolderImages(files) {
  stopWorkers();
  const imageFiles = files
    .filter((file) => file.type.startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp)$/i.test(file.name))
    .sort((a, b) => a.webkitRelativePath.localeCompare(b.webkitRelativePath));
  if (!imageFiles.length) {
    state.images = [];
    dom.imageStatus.textContent = "No supported images found in that folder.";
    refreshStats();
    return;
  }

  const renderSize = readRenderSize();
  dom.statusText.textContent = "LOADING IMAGES";
  dom.imageStatus.textContent = `Loading ${imageFiles.length} images...`;
  state.images = [];
  for (const file of imageFiles) {
    try {
      state.images.push(await prepareSourceImage(file, renderSize));
    } catch (error) {
      console.warn(`Could not load ${file.name}`, error);
    }
  }
  dom.imageStatus.textContent = `${state.images.length} images loaded at ${renderSize}px max.`;
  dom.statusText.textContent = "IDLE";
  refreshStats();
}

async function prepareSourceImage(file, maxDimension) {
  const bitmap = await createImageBitmap(file);
  const fitted = fitWithin(bitmap.width, bitmap.height, maxDimension);
  const canvas = document.createElement("canvas");
  canvas.width = fitted.width;
  canvas.height = fitted.height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  context.drawImage(bitmap, 0, 0, fitted.width, fitted.height);
  bitmap.close?.();
  const data = context.getImageData(0, 0, fitted.width, fitted.height).data;
  const relativePath = file.webkitRelativePath || file.name;
  return {
    id: safeFilename(relativePath) || safeFilename(file.name),
    name: file.name,
    path: relativePath,
    width: fitted.width,
    height: fitted.height,
    data: new Uint8ClampedArray(data)
  };
}

function generateBatch() {
  if (!state.images.length) {
    dom.batchStatus.textContent = "Pick an image folder first.";
    return;
  }
  const families = selectedFamilies();
  if (!families.length) {
    dom.batchStatus.textContent = "Select at least one randomizer.";
    return;
  }

  stopWorkers();
  const count = readBatchSize();
  const mode = dom.randomMode.value || "damaged";
  state.presets = Array.from({ length: count }, (_, index) =>
    createLabPreset(index, families[index % families.length], mode)
  );
  dom.emptyState.hidden = true;
  renderPresetCards();
  startRendering();
  refreshStats();
}

function createLabPreset(index, family, mode) {
  const label = familyLabel(family);
  const base = createPreset({
    name: `Lab ${label} ${String(index + 1).padStart(3, "0")}`,
    description: `${label} random preset generated in Preset Lab.`,
    tags: [PRESET_TAG, family],
    seed: Math.floor(Math.random() * 2147483647),
    macros: {
      bend: 0,
      colorFault: 0,
      melt: 0,
      burn: 0,
      noise: 0,
      cheapness: 0,
      chaos: 0
    }
  });
  const preset = randomizePreset(base, family, mode);
  preset.name = `Lab ${label} ${String(index + 1).padStart(3, "0")}`;
  preset.cameraModel = preset.name;
  preset.description = `${label} random preset generated in Preset Lab.`;
  preset.tags = uniqueTags([PRESET_TAG, family, ...preset.tags]);
  return {
    id: `lab-${Date.now()}-${index}`,
    family,
    mode,
    kept: false,
    preset,
    card: null,
    canvases: []
  };
}

function renderPresetCards() {
  dom.grid.innerHTML = "";
  state.presets.forEach((item, index) => {
    const card = document.createElement("article");
    card.className = "lab-preset-card";
    card.innerHTML = `
      <div class="lab-preset-meta">
        <label class="lab-keep-toggle">
          <input type="checkbox">
          <span>Keep</span>
        </label>
        <span class="lab-preset-source">#${String(index + 1).padStart(3, "0")} · ${familyLabel(item.family)} · ${modeLabel(item.mode)}</span>
      </div>
      <div class="lab-thumbs"></div>
      <div class="lab-card-actions" ${state.images.length > COLLAPSED_THUMB_COUNT ? "" : "hidden"}>
        <button type="button" class="lab-expand-button">SHOW ALL ${state.images.length}</button>
      </div>
      <details class="lab-edit-details">
        <summary>Edit name, description, tags</summary>
        <div class="lab-edit-fields">
          <label class="lab-text-field">
            <span>Name</span>
            <input class="lab-name-input" type="text" value="${escapeAttribute(item.preset.name)}">
          </label>
          <label class="lab-text-field">
            <span>Description</span>
            <textarea class="lab-description-input" rows="2">${escapeHtml(item.preset.description || "")}</textarea>
          </label>
          <label class="lab-text-field">
            <span>Tags</span>
            <input class="lab-tags-input" type="text" value="${escapeAttribute(item.preset.tags.join(", "))}">
          </label>
        </div>
      </details>
    `;
    const keepInput = card.querySelector(".lab-keep-toggle input");
    const nameInput = card.querySelector(".lab-name-input");
    const descriptionInput = card.querySelector(".lab-description-input");
    const tagsInput = card.querySelector(".lab-tags-input");
    const expandButton = card.querySelector(".lab-expand-button");
    const thumbs = card.querySelector(".lab-thumbs");

    keepInput.addEventListener("change", () => {
      item.kept = keepInput.checked;
      card.classList.toggle("is-kept", item.kept);
      refreshStats();
    });
    nameInput.addEventListener("input", () => {
      item.preset.name = nameInput.value.trim() || `Lab Preset ${index + 1}`;
      item.preset.cameraModel = item.preset.name;
    });
    descriptionInput.addEventListener("input", () => {
      item.preset.description = descriptionInput.value;
    });
    tagsInput.addEventListener("input", () => {
      item.preset.tags = uniqueTags(tagsInput.value.split(",").map((tag) => tag.trim()));
    });
    expandButton.addEventListener("click", () => {
      const expanded = card.classList.toggle("is-expanded");
      expandButton.textContent = expanded ? "SHOW 4" : `SHOW ALL ${state.images.length}`;
    });

    item.canvases = state.images.map((source, imageIndex) => {
      const figure = document.createElement("figure");
      figure.className = "lab-thumb";
      if (imageIndex >= COLLAPSED_THUMB_COUNT) figure.classList.add("is-extra");
      figure.innerHTML = `
        <canvas width="${source.width}" height="${source.height}" aria-label="${escapeAttribute(source.name)}"></canvas>
        <figcaption title="${escapeAttribute(source.path)}">${escapeHtml(source.name)}</figcaption>
      `;
      thumbs.append(figure);
      return figure.querySelector("canvas");
    });
    item.card = card;
    dom.grid.append(card);
  });
}

function startRendering() {
  stopWorkers();
  const token = state.renderToken;
  state.pendingJobs = [];
  state.completedRenders = 0;
  state.totalRenders = state.presets.length * state.images.length;
  state.presets.forEach((item, presetIndex) => {
    state.images.forEach((source, imageIndex) => {
      state.pendingJobs.push({ presetIndex, imageIndex, source, canvas: item.canvases[imageIndex] });
    });
  });
  const workerCount = Math.min(MAX_WORKERS, navigator.hardwareConcurrency || 2, state.pendingJobs.length || 1);
  state.workers = Array.from({ length: workerCount }, () => createRenderWorker(token));
  state.workers.forEach((worker) => pumpWorker(worker, token));
  dom.statusText.textContent = "RENDERING";
  dom.batchStatus.textContent = `Rendering ${state.totalRenders} thumbnails...`;
  refreshStats();
}

function createRenderWorker(token) {
  const worker = new Worker(new URL("./render-worker.js", import.meta.url), { type: "module" });
  worker.onmessage = (event) => {
    if (token !== state.renderToken) return;
    const { error, buffer, width, height } = event.data;
    const job = worker.currentJob;
    worker.currentJob = null;
    if (error) {
      console.error(error.message, error.stack);
    } else if (job) {
      paintRender(job.canvas, width, height, buffer);
    }
    state.completedRenders += 1;
    refreshStats();
    pumpWorker(worker, token);
  };
  worker.onerror = (event) => {
    console.error(event.message || event);
    state.completedRenders += 1;
    refreshStats();
    pumpWorker(worker, token);
  };
  return worker;
}

function pumpWorker(worker, token) {
  if (token !== state.renderToken) return;
  const job = state.pendingJobs.shift();
  if (!job) {
    if (state.completedRenders >= state.totalRenders) {
      dom.statusText.textContent = "READY";
      dom.batchStatus.textContent = `Rendered ${state.totalRenders} thumbnails.`;
      stopWorkers(false);
    }
    return;
  }
  const preset = clone(state.presets[job.presetIndex].preset);
  const source = job.source;
  const data = new Uint8ClampedArray(source.data);
  worker.currentJob = job;
  worker.postMessage(
    {
      type: "lab",
      jobId: `${state.renderToken}-${job.presetIndex}-${job.imageIndex}`,
      index: 0,
      width: source.width,
      height: source.height,
      buffer: data.buffer,
      preset
    },
    [data.buffer]
  );
}

function paintRender(canvas, width, height, buffer) {
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  const image = new ImageData(new Uint8ClampedArray(buffer), width, height);
  context.putImageData(image, 0, 0);
}

function stopWorkers(invalidate = true) {
  if (invalidate) state.renderToken += 1;
  state.workers.forEach((worker) => worker.terminate());
  state.workers = [];
  state.pendingJobs = [];
}

async function exportKeptModule() {
  const moduleText = buildExportModule();
  if (!moduleText) return;
  const filename = `preset-lab-${new Date().toISOString().slice(0, 10)}.js`;
  downloadBlob(new Blob([moduleText], { type: "text/javascript;charset=utf-8" }), filename);
}

async function copyKeptEntries() {
  const entries = buildPresetEntries();
  if (!entries) return;
  try {
    await navigator.clipboard.writeText(entries);
    dom.statusText.textContent = "COPIED";
  } catch (error) {
    console.warn(error);
    dom.statusText.textContent = "COPY FAILED";
  }
}

function buildExportModule() {
  const entries = buildPresetEntries();
  if (!entries) return null;
  return `import { createPreset } from "./presets.js";\n\nexport const PRESET_LAB_PRESETS = [\n${entries}\n];\n`;
}

function buildPresetEntries() {
  const kept = state.presets.filter((item) => item.kept);
  if (!kept.length) {
    dom.statusText.textContent = "NO KEPT PRESETS";
    return null;
  }
  return kept.map((item) => `  createPreset(${JSON.stringify(presetToCreateArgs(item.preset), null, 2).replace(/\n/g, "\n  ")}),`).join("\n");
}

function presetToCreateArgs(preset) {
  return {
    name: preset.name,
    cameraModel: preset.cameraModel || preset.name,
    description: preset.description || "",
    tags: uniqueTags([PRESET_TAG, ...preset.tags]),
    seed: preset.seed,
    macros: preset.macros,
    pipeline: preset.pipeline,
    temporal: preset.temporal
  };
}

function clearKept() {
  state.presets.forEach((item) => {
    item.kept = false;
    item.card?.classList.remove("is-kept");
    const input = item.card?.querySelector(".lab-keep-toggle input");
    if (input) input.checked = false;
  });
  refreshStats();
}

function selectedFamilies() {
  return [...dom.familyList.querySelectorAll("input:checked")].map((input) => input.value);
}

function readBatchSize() {
  return Math.max(1, Math.min(500, Math.round(Number(dom.batchSizeInput.value) || DEFAULT_BATCH_SIZE)));
}

function readRenderSize() {
  return Math.max(120, Math.min(900, Math.round(Number(dom.renderSizeInput.value) || DEFAULT_RENDER_SIZE)));
}

function refreshStats() {
  const kept = state.presets.filter((item) => item.kept).length;
  dom.presetCount.textContent = `${state.presets.length} preset${state.presets.length === 1 ? "" : "s"}`;
  dom.renderCount.textContent = `${state.completedRenders}/${state.totalRenders} renders`;
  dom.keptCount.textContent = `${kept} kept`;
  dom.exportKeptButton.disabled = kept === 0;
  dom.copyKeptButton.disabled = kept === 0;
  dom.clearKeptButton.disabled = kept === 0;
}

function familyLabel(value) {
  return RANDOM_FAMILIES.find(([family]) => family === value)?.[1] || value;
}

function modeLabel(value) {
  return RANDOM_MODES.find(([mode]) => mode === value)?.[1] || value;
}

function uniqueTags(tags) {
  const seen = new Set();
  return tags
    .map((tag) => String(tag || "").trim().toLowerCase())
    .filter((tag) => {
      if (!tag || seen.has(tag)) return false;
      seen.add(tag);
      return true;
    });
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function escapeAttribute(value) {
  return escapeHtml(value).replace(/"/g, "&quot;");
}

init();
