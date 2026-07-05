import { processCircuitBendImageData } from "./engine-core.js";
import {
  ADVANCED_DEFS,
  applyMacrosToPipeline,
  BUILT_IN_PRESETS,
  clonePreset,
  MACRO_DEFS,
  normalizePreset
} from "./presets.js";
import { RANDOM_FAMILIES, RANDOM_MODES, randomizeModule, randomizePreset } from "./randomize.js";
import { downloadBlob, fitWithin, getAtPath, safeFilename, setAtPath } from "./utils.js";

const PREVIEW_MAX_DIMENSION = 1500;
const THUMB_MAX_DIMENSION = 110;
const HISTORY_LIMIT = 60;

const MODULE_KEYS = ADVANCED_DEFS.map((group) => group.key);

const state = {
  source: null,
  sourceName: "",
  preset: clonePreset(BUILT_IN_PRESETS[0]),
  renderQueued: false,
  isRendering: false,
  lastRender: null,
  originalCanvas: null,
  comparing: false,
  splitMode: false,
  splitRatio: 0.5,
  soloModule: null,
  openGroups: new Set(),
  activePresetIndex: 0,
  pendingSnapshot: null,
  thumbsPending: false
};

const history = { undo: [], redo: [] };

const dom = {
  imageInput: document.querySelector("#imageInput"),
  presetInput: document.querySelector("#presetInput"),
  loadImageButton: document.querySelector("#loadImageButton"),
  loadPresetButton: document.querySelector("#loadPresetButton"),
  savePresetButton: document.querySelector("#savePresetButton"),
  exportButton: document.querySelector("#exportButton"),
  undoButton: document.querySelector("#undoButton"),
  redoButton: document.querySelector("#redoButton"),
  previewCanvas: document.querySelector("#previewCanvas"),
  emptyState: document.querySelector("#emptyState"),
  statusText: document.querySelector("#statusText"),
  dimensionsText: document.querySelector("#dimensionsText"),
  presetList: document.querySelector("#presetList"),
  macroControls: document.querySelector("#macroControls"),
  advancedControls: document.querySelector("#advancedControls"),
  randomFamilyList: document.querySelector("#randomFamilyList"),
  randomMode: document.querySelector("#randomMode"),
  presetName: document.querySelector("#presetName"),
  presetDescription: document.querySelector("#presetDescription"),
  exportFormat: document.querySelector("#exportFormat"),
  dropTarget: document.querySelector("#dropTarget"),
  compareButton: document.querySelector("#compareButton"),
  splitButton: document.querySelector("#splitButton"),
  seedInput: document.querySelector("#seedInput"),
  rerollButton: document.querySelector("#rerollButton"),
  helpButton: document.querySelector("#helpButton"),
  helpDialog: document.querySelector("#helpDialog"),
  helpClose: document.querySelector("#helpClose")
};

const thumbCanvases = [];
let worker = null;
let previewJobId = 0;
let exportJobId = 0;
let pendingExportFormat = null;
let thumbSource = null;

init();

function init() {
  worker = setupWorker();
  renderPresetList();
  renderRandomControls();
  renderControls();
  bindEvents();
  updateHistoryButtons();
  updateStatus("NO SIGNAL");
}

function setupWorker() {
  if (typeof Worker === "undefined") return null;
  try {
    const created = new Worker(new URL("./render-worker.js", import.meta.url), {
      type: "module"
    });
    created.onmessage = onWorkerMessage;
    created.onerror = (event) => {
      console.error("Render worker failed, falling back to main thread", event);
      worker = null;
      state.isRendering = false;
      scheduleRender();
    };
    return created;
  } catch (error) {
    console.error(error);
    return null;
  }
}

function onWorkerMessage(event) {
  const { type, jobId, index, width, height, elapsed, buffer } = event.data;
  const imageData = new ImageData(new Uint8ClampedArray(buffer), width, height);
  if (type === "preview") {
    if (jobId !== previewJobId) return;
    finishPreview(imageData, elapsed);
  } else if (type === "export") {
    if (jobId !== exportJobId) return;
    completeExport(imageData, pendingExportFormat);
  } else if (type === "thumb") {
    const canvas = thumbCanvases[index];
    if (!canvas) return;
    canvas.width = width;
    canvas.height = height;
    canvas.getContext("2d").putImageData(imageData, 0, 0);
    canvas.classList.add("has-image");
  }
}

function bindEvents() {
  dom.loadImageButton.addEventListener("click", () => dom.imageInput.click());
  dom.loadPresetButton.addEventListener("click", () => dom.presetInput.click());
  dom.savePresetButton.addEventListener("click", savePreset);
  dom.exportButton.addEventListener("click", exportImage);
  dom.undoButton.addEventListener("click", undo);
  dom.redoButton.addEventListener("click", redo);
  dom.imageInput.addEventListener("change", () => {
    const file = dom.imageInput.files?.[0];
    if (file) loadImageFile(file);
    dom.imageInput.value = "";
  });
  dom.presetInput.addEventListener("change", () => {
    const file = dom.presetInput.files?.[0];
    if (file) loadPresetFile(file);
    dom.presetInput.value = "";
  });
  dom.presetName.addEventListener("input", () => {
    state.preset.name = dom.presetName.value || "Untitled Bend";
    state.preset.cameraModel = state.preset.name;
  });
  dom.presetDescription.addEventListener("input", () => {
    state.preset.description = dom.presetDescription.value;
  });
  dom.exportFormat.addEventListener("change", () => {
    state.preset.pipeline.output.format = dom.exportFormat.value;
  });
  dom.dropTarget.addEventListener("dragover", (event) => {
    event.preventDefault();
    dom.dropTarget.classList.add("is-dragging");
  });
  dom.dropTarget.addEventListener("dragleave", () => {
    dom.dropTarget.classList.remove("is-dragging");
  });
  dom.dropTarget.addEventListener("drop", (event) => {
    event.preventDefault();
    dom.dropTarget.classList.remove("is-dragging");
    const file = [...event.dataTransfer.files].find((item) =>
      item.type.startsWith("image/")
    );
    if (file) loadImageFile(file);
  });

  dom.seedInput.addEventListener("change", () => {
    const parsed = Number.parseInt(dom.seedInput.value, 10);
    if (Number.isFinite(parsed)) {
      pushHistory(snapshotPreset());
      state.preset.seed = parsed;
      scheduleRender();
    } else {
      dom.seedInput.value = String(state.preset.seed);
    }
  });
  dom.rerollButton.addEventListener("click", rerollSeed);

  dom.helpButton.addEventListener("click", () => dom.helpDialog.showModal());
  dom.helpClose.addEventListener("click", () => dom.helpDialog.close());
  dom.helpDialog.addEventListener("click", (event) => {
    if (event.target === dom.helpDialog) dom.helpDialog.close();
  });

  dom.compareButton.addEventListener("pointerdown", () => setComparing(true));
  dom.compareButton.addEventListener("pointerup", () => setComparing(false));
  dom.compareButton.addEventListener("pointerleave", () => setComparing(false));
  dom.splitButton.addEventListener("click", toggleSplitMode);

  dom.previewCanvas.addEventListener("pointerdown", (event) => {
    if (!state.splitMode) return;
    dom.previewCanvas.setPointerCapture(event.pointerId);
    updateSplitFromPointer(event);
  });
  dom.previewCanvas.addEventListener("pointermove", (event) => {
    if (!state.splitMode || event.buttons === 0) return;
    updateSplitFromPointer(event);
  });

  window.addEventListener("keydown", (event) => {
    if (isTypingTarget(event.target)) return;
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "z") {
      event.preventDefault();
      if (event.shiftKey) redo();
      else undo();
      return;
    }
    if (event.repeat) return;
    if (event.key === "?") {
      if (dom.helpDialog.open) dom.helpDialog.close();
      else dom.helpDialog.showModal();
      return;
    }
    if (event.key === "c" || event.key === "C") setComparing(true);
    if (event.key === "s" || event.key === "S") toggleSplitMode();
    if (event.key === "r" || event.key === "R") {
      pushHistory(snapshotPreset());
      state.preset = randomizePreset(state.preset, "global", dom.randomMode.value);
      setActivePreset(-1);
      renderControls();
      scheduleRender();
    }
  });
  window.addEventListener("keyup", (event) => {
    if (event.key === "c" || event.key === "C") setComparing(false);
  });
  window.addEventListener("blur", () => setComparing(false));
}

function isTypingTarget(target) {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement
  );
}

// --- History ---

function snapshotPreset() {
  return clonePreset(state.preset);
}

function pushHistory(snapshot) {
  history.undo.push(snapshot);
  if (history.undo.length > HISTORY_LIMIT) history.undo.shift();
  history.redo.length = 0;
  updateHistoryButtons();
}

function undo() {
  if (!history.undo.length) return;
  history.redo.push(snapshotPreset());
  state.preset = history.undo.pop();
  setActivePreset(-1);
  renderControls();
  scheduleRender();
  updateHistoryButtons();
  updateStatus("UNDO");
}

function redo() {
  if (!history.redo.length) return;
  history.undo.push(snapshotPreset());
  state.preset = history.redo.pop();
  setActivePreset(-1);
  renderControls();
  scheduleRender();
  updateHistoryButtons();
  updateStatus("REDO");
}

function updateHistoryButtons() {
  dom.undoButton.disabled = history.undo.length === 0;
  dom.redoButton.disabled = history.redo.length === 0;
}

function armSliderSnapshot() {
  if (!state.pendingSnapshot) state.pendingSnapshot = snapshotPreset();
}

function commitSliderSnapshot() {
  if (state.pendingSnapshot) {
    pushHistory(state.pendingSnapshot);
    state.pendingSnapshot = null;
  }
}

// --- Compare / split ---

function setComparing(active) {
  if (!state.source || state.comparing === active) return;
  state.comparing = active;
  dom.compareButton.classList.toggle("is-active", active);
  drawComposite();
  if (active) updateStatus("ORIGINAL");
  else updateStatus(`${state.preset.name.toUpperCase()} READY`);
}

function toggleSplitMode() {
  if (!state.source) return;
  state.splitMode = !state.splitMode;
  dom.splitButton.classList.toggle("is-active", state.splitMode);
  dom.dropTarget.classList.toggle("is-split", state.splitMode);
  drawComposite();
  updateStatus(state.splitMode ? "SPLIT COMPARE · DRAG IMAGE" : "SPLIT OFF");
}

function updateSplitFromPointer(event) {
  const rect = dom.previewCanvas.getBoundingClientRect();
  if (rect.width <= 0) return;
  state.splitRatio = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
  drawComposite();
}

function drawComposite() {
  const render = state.lastRender;
  if (!render) return;
  const canvas = dom.previewCanvas;
  if (canvas.width !== render.width || canvas.height !== render.height) {
    canvas.width = render.width;
    canvas.height = render.height;
  }
  const context = canvas.getContext("2d");
  context.clearRect(0, 0, canvas.width, canvas.height);

  if (state.comparing && state.originalCanvas) {
    context.drawImage(state.originalCanvas, 0, 0);
    return;
  }

  context.drawImage(render, 0, 0);

  if (state.splitMode && state.originalCanvas) {
    const splitX = Math.round(canvas.width * state.splitRatio);
    context.save();
    context.beginPath();
    context.rect(0, 0, splitX, canvas.height);
    context.clip();
    context.drawImage(state.originalCanvas, 0, 0);
    context.restore();
    context.fillStyle = "rgba(141, 255, 122, 0.9)";
    context.fillRect(splitX - 1, 0, 2, canvas.height);
    const handleY = Math.round(canvas.height / 2);
    const handleSize = Math.max(6, Math.round(canvas.width * 0.008));
    context.beginPath();
    context.moveTo(splitX - handleSize - 2, handleY);
    context.lineTo(splitX - 2, handleY - handleSize);
    context.lineTo(splitX - 2, handleY + handleSize);
    context.closePath();
    context.fill();
    context.beginPath();
    context.moveTo(splitX + handleSize + 2, handleY);
    context.lineTo(splitX + 2, handleY - handleSize);
    context.lineTo(splitX + 2, handleY + handleSize);
    context.closePath();
    context.fill();
  }
}

// --- Solo ---

function effectivePreset() {
  if (!state.soloModule) return state.preset;
  const preset = clonePreset(state.preset);
  MODULE_KEYS.forEach((key) => {
    if (key !== state.soloModule && preset.pipeline[key]) {
      preset.pipeline[key].enabled = false;
    }
  });
  return preset;
}

function toggleSolo(moduleKey, groupName) {
  state.soloModule = state.soloModule === moduleKey ? null : moduleKey;
  renderAdvancedControls();
  scheduleRender();
  updateStatus(state.soloModule ? `SOLO · ${groupName.toUpperCase()}` : "SOLO OFF");
}

// --- Presets / controls ---

function setActivePreset(index) {
  state.activePresetIndex = index;
  dom.presetList.querySelectorAll(".preset-button").forEach((button, i) => {
    button.classList.toggle("is-active", i === index);
  });
}

function renderPresetList() {
  dom.presetList.innerHTML = "";
  thumbCanvases.length = 0;
  BUILT_IN_PRESETS.forEach((preset, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "preset-button";
    const thumb = document.createElement("canvas");
    thumb.className = "preset-thumb";
    thumb.width = 4;
    thumb.height = 3;
    const text = document.createElement("span");
    text.className = "preset-text";
    text.innerHTML = `<span>${preset.name}</span><small>${preset.tags.join(" / ")}</small>`;
    button.append(thumb, text);
    button.addEventListener("click", () => {
      pushHistory(snapshotPreset());
      state.preset = clonePreset(preset);
      setActivePreset(index);
      renderControls();
      scheduleRender();
    });
    thumbCanvases.push(thumb);
    dom.presetList.append(button);
  });
  setActivePreset(state.activePresetIndex);
}

function renderRandomControls() {
  dom.randomMode.innerHTML = "";
  RANDOM_MODES.forEach(([value, label, description]) => {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label;
    option.title = description || "";
    if (value === "bent") option.selected = true;
    dom.randomMode.append(option);
  });
  const syncModeTooltip = () => {
    const selected = RANDOM_MODES.find(([value]) => value === dom.randomMode.value);
    dom.randomMode.title = selected?.[2] || "";
  };
  syncModeTooltip();
  dom.randomMode.addEventListener("change", syncModeTooltip);

  dom.randomFamilyList.innerHTML = "";
  RANDOM_FAMILIES.forEach(([family, label, description]) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = family === "global" ? "command-button is-primary" : "command-button";
    button.textContent = label;
    button.title = description || "";
    button.addEventListener("click", () => {
      pushHistory(snapshotPreset());
      state.preset = randomizePreset(state.preset, family, dom.randomMode.value);
      setActivePreset(-1);
      renderControls();
      scheduleRender();
    });
    dom.randomFamilyList.append(button);
  });
}

function renderControls() {
  dom.presetName.value = state.preset.name;
  dom.presetDescription.value = state.preset.description || "";
  dom.exportFormat.value = state.preset.pipeline.output.format || "png";
  dom.seedInput.value = String(state.preset.seed);
  renderMacroControls();
  renderAdvancedControls();
}

function renderMacroControls() {
  dom.macroControls.innerHTML = "";
  MACRO_DEFS.forEach(([key, label]) => {
    const value = Number(state.preset.macros[key] || 0);
    const row = document.createElement("label");
    row.className = "control-row";
    row.innerHTML = `
      <span>${label}</span>
      <input type="range" min="0" max="1" step="0.01" value="${value}">
      <output>${formatPercent(value)}</output>
    `;
    const input = row.querySelector("input");
    const output = row.querySelector("output");
    input.addEventListener("pointerdown", armSliderSnapshot);
    input.addEventListener("keydown", armSliderSnapshot);
    input.addEventListener("change", commitSliderSnapshot);
    input.addEventListener("input", () => {
      const nextValue = Number(input.value);
      state.preset.macros[key] = nextValue;
      output.textContent = formatPercent(nextValue);
      applyMacrosToPipeline(state.preset);
      renderAdvancedControls();
      scheduleRender();
    });
    dom.macroControls.append(row);
  });
}

function renderAdvancedControls() {
  dom.advancedControls.innerHTML = "";
  ADVANCED_DEFS.forEach((group) => {
    const details = document.createElement("details");
    details.className = "advanced-group";
    if (state.soloModule) {
      details.classList.toggle("is-solo", state.soloModule === group.key);
      details.classList.toggle("is-dimmed", state.soloModule !== group.key);
    }
    details.open = state.openGroups.has(group.group);
    details.addEventListener("toggle", () => {
      if (details.open) state.openGroups.add(group.group);
      else state.openGroups.delete(group.group);
    });

    const summary = document.createElement("summary");
    const enabledPath = group.controls.find(([, , type]) => type === "boolean")?.[0];
    summary.innerHTML = `
      <span>${group.group}</span>
      <span class="summary-tools">
        <button type="button" class="dice-button" title="Randomize this module (uses Randomize mode)">R</button>
        <button type="button" class="solo-button" title="Solo this module">S</button>
        <button type="button" class="group-lamp" title="Toggle module on/off"></button>
      </span>
    `;
    details.append(summary);

    const diceButton = summary.querySelector(".dice-button");
    diceButton.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      pushHistory(snapshotPreset());
      state.preset = randomizeModule(state.preset, group.key, dom.randomMode.value);
      renderAdvancedControls();
      scheduleRender();
      updateStatus(`${group.group.toUpperCase()} RANDOMIZED`);
    });

    const soloButton = summary.querySelector(".solo-button");
    soloButton.classList.toggle("is-active", state.soloModule === group.key);
    soloButton.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      toggleSolo(group.key, group.group);
    });

    const lamp = summary.querySelector(".group-lamp");
    const refreshLamp = () => {
      const on = enabledPath ? Boolean(getAtPath(state.preset, enabledPath)) : true;
      lamp.classList.toggle("is-on", on);
    };
    refreshLamp();
    lamp.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      if (!enabledPath) return;
      pushHistory(snapshotPreset());
      setAtPath(state.preset, enabledPath, !getAtPath(state.preset, enabledPath));
      renderAdvancedControls();
      scheduleRender();
    });

    group.controls.forEach(([path, label, type, min, max, step]) => {
      const value = getAtPath(state.preset, path);
      const row = document.createElement("label");

      if (type === "boolean") {
        row.className = "control-row is-toggle";
        row.innerHTML = `
          <span>${label}</span>
          <input type="checkbox" ${value ? "checked" : ""}>
        `;
        const input = row.querySelector("input");
        input.addEventListener("change", () => {
          pushHistory(snapshotPreset());
          setAtPath(state.preset, path, input.checked);
          refreshLamp();
          scheduleRender();
        });
      } else if (type === "select") {
        row.className = "control-row is-select";
        const options = min;
        row.innerHTML = `
          <span>${label}</span>
          <select>
            ${options
              .map(
                (option) =>
                  `<option value="${option}" ${option === value ? "selected" : ""}>${option}</option>`
              )
              .join("")}
          </select>
        `;
        const select = row.querySelector("select");
        select.addEventListener("change", () => {
          pushHistory(snapshotPreset());
          setAtPath(state.preset, path, select.value);
          scheduleRender();
        });
      } else {
        row.className = "control-row";
        row.innerHTML = `
          <span>${label}</span>
          <input type="range" min="${min}" max="${max}" step="${step}" value="${value}">
          <output>${formatControlValue(value)}</output>
        `;
        const input = row.querySelector("input");
        const output = row.querySelector("output");
        input.addEventListener("pointerdown", armSliderSnapshot);
        input.addEventListener("keydown", armSliderSnapshot);
        input.addEventListener("change", commitSliderSnapshot);
        input.addEventListener("input", () => {
          const numericValue = step >= 1 ? Math.round(Number(input.value)) : Number(input.value);
          setAtPath(state.preset, path, numericValue);
          output.textContent = formatControlValue(numericValue);
          scheduleRender();
        });
      }

      details.append(row);
    });

    dom.advancedControls.append(details);
  });
}

// --- Image / preset IO ---

async function loadImageFile(file) {
  updateStatus("LOADING IMAGE");
  try {
    state.source = await fileToImageSource(file);
    state.sourceName = file.name;
    dom.dimensionsText.textContent = `${state.source.width} x ${state.source.height}`;
    dom.emptyState.hidden = true;
    prepareThumbSource();
    state.thumbsPending = true;
    scheduleRender();
  } catch (error) {
    console.error(error);
    updateStatus("IMAGE LOAD FAILED");
  }
}

async function loadPresetFile(file) {
  updateStatus("LOADING PRESET");
  try {
    const json = JSON.parse(await file.text());
    pushHistory(snapshotPreset());
    state.preset = normalizePreset(json);
    setActivePreset(-1);
    renderControls();
    scheduleRender();
    updateStatus("PRESET LOADED");
  } catch (error) {
    console.error(error);
    updateStatus("PRESET LOAD FAILED");
  }
}

function rerollSeed() {
  pushHistory(snapshotPreset());
  state.preset.seed = Math.floor(Math.random() * 2147483647);
  dom.seedInput.value = String(state.preset.seed);
  scheduleRender();
}

// --- Rendering ---

function sourceSize() {
  return {
    width: state.source.width || state.source.naturalWidth,
    height: state.source.height || state.source.naturalHeight
  };
}

function refreshOriginalCanvas(width, height) {
  if (!state.originalCanvas) state.originalCanvas = document.createElement("canvas");
  const canvas = state.originalCanvas;
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  context.clearRect(0, 0, width, height);
  context.drawImage(state.source, 0, 0, width, height);
  return context;
}

function scheduleRender() {
  if (!state.source) return;
  if (state.isRendering) {
    state.renderQueued = true;
    return;
  }
  if (state.renderQueued) return;
  state.renderQueued = true;
  requestAnimationFrame(() => {
    state.renderQueued = false;
    renderPreview();
  });
}

function renderPreview() {
  if (!state.source) return;
  state.isRendering = true;
  updateStatus(state.soloModule ? "PROCESSING · SOLO" : "PROCESSING");
  try {
    const size = sourceSize();
    const fitted = fitWithin(size.width, size.height, PREVIEW_MAX_DIMENSION);
    const context = refreshOriginalCanvas(fitted.width, fitted.height);
    const imageData = context.getImageData(0, 0, fitted.width, fitted.height);
    const preset = effectivePreset();

    if (worker) {
      previewJobId += 1;
      worker.postMessage(
        {
          type: "preview",
          jobId: previewJobId,
          width: imageData.width,
          height: imageData.height,
          buffer: imageData.data.buffer,
          preset
        },
        [imageData.data.buffer]
      );
    } else {
      const startedAt = performance.now();
      processCircuitBendImageData(imageData, preset);
      finishPreview(imageData, Math.round(performance.now() - startedAt));
    }
  } catch (error) {
    console.error(error);
    state.isRendering = false;
    updateStatus("RENDER FAILED");
  }
}

function finishPreview(imageData, elapsed) {
  if (!state.lastRender) state.lastRender = document.createElement("canvas");
  state.lastRender.width = imageData.width;
  state.lastRender.height = imageData.height;
  state.lastRender.getContext("2d").putImageData(imageData, 0, 0);
  drawComposite();
  const soloTag = state.soloModule ? " · SOLO" : "";
  updateStatus(`${state.preset.name.toUpperCase()} READY · ${elapsed}MS${soloTag}`);
  state.isRendering = false;
  if (state.renderQueued) {
    state.renderQueued = false;
    scheduleRender();
  }
  if (state.thumbsPending) {
    state.thumbsPending = false;
    generateThumbnails();
  }
}

// --- Export ---

async function exportImage() {
  if (!state.source) {
    updateStatus("NO IMAGE");
    return;
  }
  updateStatus("EXPORTING");
  await nextFrame();
  try {
    const size = sourceSize();
    const canvas = document.createElement("canvas");
    canvas.width = size.width;
    canvas.height = size.height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    context.drawImage(state.source, 0, 0, size.width, size.height);
    const imageData = context.getImageData(0, 0, size.width, size.height);
    const preset = effectivePreset();
    const format = state.preset.pipeline.output.format || "png";

    if (worker) {
      exportJobId += 1;
      pendingExportFormat = format;
      worker.postMessage(
        {
          type: "export",
          jobId: exportJobId,
          width: imageData.width,
          height: imageData.height,
          buffer: imageData.data.buffer,
          preset
        },
        [imageData.data.buffer]
      );
    } else {
      processCircuitBendImageData(imageData, preset);
      await completeExport(imageData, format);
    }
  } catch (error) {
    console.error(error);
    updateStatus("EXPORT FAILED");
  }
}

async function completeExport(imageData, format) {
  try {
    pendingExportFormat = null;
    const canvas = document.createElement("canvas");
    canvas.width = imageData.width;
    canvas.height = imageData.height;
    canvas.getContext("2d").putImageData(imageData, 0, 0);
    const exportFormat = format || "png";
    const mime = `image/${exportFormat === "jpg" ? "jpeg" : exportFormat}`;
    const blob = await canvasToBlob(canvas, mime, 0.95);
    downloadBlob(blob, `${safeFilename(state.preset.name)}.${exportFormat}`);
    updateStatus("EXPORTED");
  } catch (error) {
    console.error(error);
    updateStatus("EXPORT FAILED");
  }
}

// --- Thumbnails ---

function prepareThumbSource() {
  const size = sourceSize();
  const fitted = fitWithin(size.width, size.height, THUMB_MAX_DIMENSION);
  const canvas = document.createElement("canvas");
  canvas.width = fitted.width;
  canvas.height = fitted.height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  context.drawImage(state.source, 0, 0, fitted.width, fitted.height);
  thumbSource = context.getImageData(0, 0, fitted.width, fitted.height);
}

function generateThumbnails() {
  if (!thumbSource) return;
  BUILT_IN_PRESETS.forEach((preset, index) => {
    if (worker) {
      const copy = new Uint8ClampedArray(thumbSource.data);
      worker.postMessage(
        {
          type: "thumb",
          index,
          width: thumbSource.width,
          height: thumbSource.height,
          buffer: copy.buffer,
          preset
        },
        [copy.buffer]
      );
    } else {
      const image = {
        width: thumbSource.width,
        height: thumbSource.height,
        data: new Uint8ClampedArray(thumbSource.data)
      };
      processCircuitBendImageData(image, preset);
      const canvas = thumbCanvases[index];
      if (!canvas) return;
      canvas.width = image.width;
      canvas.height = image.height;
      canvas
        .getContext("2d")
        .putImageData(new ImageData(image.data, image.width, image.height), 0, 0);
      canvas.classList.add("has-image");
    }
  });
}

// --- Preset save ---

async function savePreset() {
  const preset = clonePreset(state.preset);
  preset.name = dom.presetName.value || preset.name;
  preset.cameraModel = preset.name;
  preset.description = dom.presetDescription.value;
  preset.createdAt = new Date().toISOString();
  preset.thumbnail = await makeThumbnail();
  const blob = new Blob([JSON.stringify(preset, null, 2)], {
    type: "application/json"
  });
  downloadBlob(blob, `${safeFilename(preset.name)}.vcb-preset.json`);
  updateStatus("PRESET SAVED");
}

async function makeThumbnail() {
  if (!state.lastRender?.width) return null;
  const max = 240;
  const scale = Math.min(1, max / Math.max(state.lastRender.width, state.lastRender.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(state.lastRender.width * scale));
  canvas.height = Math.max(1, Math.round(state.lastRender.height * scale));
  const context = canvas.getContext("2d");
  context.drawImage(state.lastRender, 0, 0, canvas.width, canvas.height);
  return {
    type: "data-url",
    mime: "image/webp",
    data: canvas.toDataURL("image/webp", 0.72)
  };
}

// --- Helpers ---

async function fileToImageSource(file) {
  if ("createImageBitmap" in window) {
    return createImageBitmap(file);
  }
  const url = URL.createObjectURL(file);
  try {
    return await new Promise((resolve, reject) => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.onerror = reject;
      image.src = url;
    });
  } finally {
    URL.revokeObjectURL(url);
  }
}

function canvasToBlob(canvas, mime, quality) {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error("Canvas export failed"));
    }, mime, quality);
  });
}

function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

function updateStatus(value) {
  dom.statusText.textContent = value;
}

function formatPercent(value) {
  return `${Math.round(Number(value) * 100)}`;
}

function formatControlValue(value) {
  if (typeof value !== "number") return value;
  return Number.isInteger(value) ? String(value) : value.toFixed(2);
}
