import { processCircuitBendImageData } from "./engine-core.js";
import {
  ADVANCED_CONTROL_HELP,
  ADVANCED_DEFS,
  applyMacrosToPipeline,
  BUILT_IN_PRESETS,
  clonePreset,
  MACRO_DEFS,
  normalizePreset
} from "./presets.js";
import { RANDOM_FAMILIES, RANDOM_MODES, randomizeModule, randomizePreset } from "./randomize.js";
import {
  clone,
  createBlobWriter,
  downloadBlob,
  fitWithin,
  getAtPath,
  safeFilename,
  setAtPath
} from "./utils.js";

const PREVIEW_MAX_DIMENSION = 1500;
const THUMB_MAX_DIMENSION = 110;
const GHOST_EXPORT_MAX_DIMENSION = 3000;
const HISTORY_LIMIT = 60;

const MODULE_KEYS = ADVANCED_DEFS.map((group) => group.key);

const state = {
  source: null,
  sourceName: "",
  ghostSource: null,
  ghostName: "",
  ghostPreview: null,
  ghostThumb: null,
  preset: clonePreset(BUILT_IN_PRESETS[0]),
  renderQueued: false,
  isRendering: false,
  lastRender: null,
  originalCanvas: null,
  comparing: false,
  splitMode: false,
  splitRatio: 0.5,
  soloModule: null,
  frozenModules: new Set(),
  freezeSeed: false,
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
  exportSize: document.querySelector("#exportSize"),
  dropTarget: document.querySelector("#dropTarget"),
  compareButton: document.querySelector("#compareButton"),
  splitButton: document.querySelector("#splitButton"),
  seedInput: document.querySelector("#seedInput"),
  rerollButton: document.querySelector("#rerollButton"),
  seedLockButton: document.querySelector("#seedLockButton"),
  helpButton: document.querySelector("#helpButton"),
  helpDialog: document.querySelector("#helpDialog"),
  helpClose: document.querySelector("#helpClose")
};

const thumbCanvases = [];
let worker = null;
let previewJobId = 0;
let exportJobId = 0;
let pendingExportRequest = null;
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
  const { type, jobId, index, width, height, elapsed, buffer, error } = event.data;
  if (error) {
    handleWorkerError(type, jobId, error);
    return;
  }
  const imageData = new ImageData(new Uint8ClampedArray(buffer), width, height);
  if (type === "preview") {
    if (jobId !== previewJobId) return;
    finishPreview(imageData, elapsed);
  } else if (type === "export") {
    if (jobId !== exportJobId) return;
    completeExport(imageData, pendingExportRequest);
  } else if (type === "thumb") {
    const canvas = thumbCanvases[index];
    if (!canvas) return;
    canvas.width = width;
    canvas.height = height;
    canvas.getContext("2d").putImageData(imageData, 0, 0);
    canvas.classList.add("has-image");
  }
}

function handleWorkerError(type, jobId, error) {
  console.error(`Render worker ${type} job failed`, error);
  if (type === "export" && jobId === exportJobId) {
    pendingExportRequest = null;
    updateStatus("EXPORT FAILED");
  } else if (type === "preview" && jobId === previewJobId) {
    state.isRendering = false;
    updateStatus("RENDER FAILED");
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
  dom.exportSize.addEventListener("change", () => {
    state.preset.pipeline.output.preserveOriginalResolution = dom.exportSize.value === "full";
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
  dom.seedLockButton.addEventListener("click", toggleSeedFreeze);

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
      state.preset = randomizePresetWithLocks(family, dom.randomMode.value);
      setActivePreset(-1);
      renderControls();
      if (state.frozenModules.size) flashFrozenModuleLocks();
      scheduleRender();
      updateStatus(`${label.toUpperCase()} RANDOMIZED${state.frozenModules.size ? " · LOCKS HELD" : ""}`);
    });
    dom.randomFamilyList.append(button);
  });
}

function randomizePresetWithLocks(family, modeName) {
  const previousSeed = state.preset.seed;
  const frozenModules = snapshotFrozenModules(state.preset);
  const nextPreset = randomizePreset(state.preset, family, modeName);
  restoreFrozenModules(nextPreset, frozenModules);
  if (state.freezeSeed) nextPreset.seed = previousSeed;
  return nextPreset;
}

function snapshotFrozenModules(preset) {
  const modules = {};
  state.frozenModules.forEach((key) => {
    if (preset.pipeline[key]) modules[key] = clone(preset.pipeline[key]);
  });
  return modules;
}

function restoreFrozenModules(preset, modules) {
  Object.entries(modules).forEach(([key, moduleConfig]) => {
    if (preset.pipeline[key]) preset.pipeline[key] = clone(moduleConfig);
  });
}

function toggleModuleFreeze(moduleKey, groupName) {
  if (state.frozenModules.has(moduleKey)) {
    state.frozenModules.delete(moduleKey);
    updateStatus(`${groupName.toUpperCase()} UNFROZEN`);
  } else {
    state.frozenModules.add(moduleKey);
    updateStatus(`${groupName.toUpperCase()} FROZEN`);
  }
  renderAdvancedControls();
}

function flashModuleLock(moduleKey) {
  const button = dom.advancedControls.querySelector(`[data-module-key="${moduleKey}"] .lock-button`);
  if (!button) return;
  button.classList.remove("is-flashing");
  void button.offsetWidth;
  button.classList.add("is-flashing");
  window.setTimeout(() => button.classList.remove("is-flashing"), 650);
}

function flashFrozenModuleLocks() {
  state.frozenModules.forEach((moduleKey) => flashModuleLock(moduleKey));
}

function renderControls() {
  dom.presetName.value = state.preset.name;
  dom.presetDescription.value = state.preset.description || "";
  dom.exportFormat.value = state.preset.pipeline.output.format || "png";
  dom.exportSize.value = state.preset.pipeline.output.preserveOriginalResolution ? "full" : "preview";
  dom.seedInput.value = String(state.preset.seed);
  refreshSeedLockButton();
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
    const moduleFrozen = state.frozenModules.has(group.key);
    const details = document.createElement("details");
    details.className = "advanced-group";
    details.dataset.moduleKey = group.key;
    details.classList.toggle("is-frozen", moduleFrozen);
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
        <button type="button" class="lock-button" title="${moduleFrozen ? "Module frozen during randomize" : "Freeze this module during randomize"}" aria-label="${moduleFrozen ? "Unfreeze" : "Freeze"} ${group.group}" aria-pressed="${moduleFrozen}">${lockIconSvg(moduleFrozen)}</button>
        <button type="button" class="dice-button" title="Randomize this module (uses Randomize mode)">R</button>
        <button type="button" class="solo-button" title="Solo this module">S</button>
        <button type="button" class="group-lamp" title="Toggle module on/off"></button>
      </span>
    `;
    details.append(summary);

    const lockButton = summary.querySelector(".lock-button");
    lockButton.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      toggleModuleFreeze(group.key, group.group);
    });

    const diceButton = summary.querySelector(".dice-button");
    diceButton.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      if (state.frozenModules.has(group.key)) {
        flashModuleLock(group.key);
        updateStatus(`${group.group.toUpperCase()} FROZEN`);
        return;
      }
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
      const tooltip = ADVANCED_CONTROL_HELP[path] || `${label} parameter.`;
      row.title = tooltip;

      if (type === "boolean") {
        row.className = "control-row is-toggle";
        row.innerHTML = `
          <span>${label}</span>
          <input type="checkbox" ${value ? "checked" : ""}>
        `;
        const input = row.querySelector("input");
        input.title = tooltip;
        input.addEventListener("change", () => {
          pushHistory(snapshotPreset());
          setAtPath(state.preset, path, input.checked);
          refreshLamp();
          scheduleRender();
        });
      } else if (type === "ghost") {
        // Not a preset field: the ghost image is session state, like the
        // source photo. bufferGhost falls back to a self-frame without it.
        row.className = "control-row is-ghost";
        row.innerHTML = `
          <span>${label}</span>
          <output>${state.ghostName || "self-frame"}</output>
          <button type="button" class="ghost-load" title="Load a second image as the stale frame">LOAD</button>
          <button type="button" class="ghost-clear" title="Clear ghost image" ${state.ghostSource ? "" : "hidden"}>✕</button>
        `;
        const ghostInput = document.createElement("input");
        const ghostOutput = row.querySelector("output");
        ghostOutput.title = tooltip;
        ghostInput.type = "file";
        ghostInput.accept = "image/*";
        ghostInput.hidden = true;
        row.append(ghostInput);
        row.querySelector(".ghost-load").addEventListener("click", (event) => {
          event.preventDefault();
          ghostInput.click();
        });
        ghostInput.addEventListener("change", () => {
          const file = ghostInput.files?.[0];
          if (file) loadGhostFile(file);
        });
        row.querySelector(".ghost-clear").addEventListener("click", (event) => {
          event.preventDefault();
          clearGhost();
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
        select.title = tooltip;
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
        input.title = tooltip;
        output.title = tooltip;
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

async function loadGhostFile(file) {
  updateStatus("LOADING GHOST");
  try {
    state.ghostSource = await fileToImageSource(file);
    state.ghostName = file.name;
    state.ghostPreview = rasterizeGhost(PREVIEW_MAX_DIMENSION);
    state.ghostThumb = rasterizeGhost(THUMB_MAX_DIMENSION);
    renderAdvancedControls();
    scheduleRender();
    updateStatus("GHOST LOADED");
  } catch (error) {
    console.error(error);
    updateStatus("GHOST LOAD FAILED");
  }
}

function clearGhost() {
  state.ghostSource = null;
  state.ghostName = "";
  state.ghostPreview = null;
  state.ghostThumb = null;
  renderAdvancedControls();
  scheduleRender();
  updateStatus("GHOST CLEARED");
}

function rasterizeGhost(maxDimension) {
  const width = state.ghostSource.width || state.ghostSource.naturalWidth;
  const height = state.ghostSource.height || state.ghostSource.naturalHeight;
  const fitted = fitWithin(width, height, maxDimension);
  const canvas = document.createElement("canvas");
  canvas.width = fitted.width;
  canvas.height = fitted.height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  context.drawImage(state.ghostSource, 0, 0, fitted.width, fitted.height);
  return context.getImageData(0, 0, fitted.width, fitted.height);
}

// Worker payload: the buffer is intentionally NOT transferred, so the
// cached ghost survives repeated renders (structured clone copies it).
function ghostPayload(imageData) {
  if (!imageData) return null;
  return { width: imageData.width, height: imageData.height, buffer: imageData.data.buffer };
}

function ghostResources(imageData) {
  if (!imageData) return {};
  return { ghost: { width: imageData.width, height: imageData.height, data: imageData.data } };
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

function toggleSeedFreeze() {
  state.freezeSeed = !state.freezeSeed;
  refreshSeedLockButton();
  updateStatus(state.freezeSeed ? "SEED FROZEN" : "SEED UNFROZEN");
}

function refreshSeedLockButton() {
  dom.seedLockButton.innerHTML = lockIconSvg(state.freezeSeed);
  dom.seedLockButton.classList.toggle("is-active", state.freezeSeed);
  dom.seedLockButton.setAttribute("aria-pressed", String(state.freezeSeed));
  dom.seedLockButton.title = state.freezeSeed
    ? "Seed is frozen during randomize"
    : "Freeze seed during randomize";
  dom.seedLockButton.setAttribute(
    "aria-label",
    state.freezeSeed ? "Unfreeze seed during randomize" : "Freeze seed during randomize"
  );
}

function lockIconSvg(locked) {
  const shackle = locked
    ? '<path d="M7 10V7a5 5 0 0 1 10 0v3" />'
    : '<path d="M8 10V7a5 5 0 0 1 9.5-2.2" />';
  return `
    <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <rect x="5" y="10" width="14" height="10" rx="2" />
      ${shackle}
      <path d="M12 14v3" />
    </svg>
  `;
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
          preset,
          ghost: ghostPayload(state.ghostPreview)
        },
        [imageData.data.buffer]
      );
    } else {
      const startedAt = performance.now();
      processCircuitBendImageData(imageData, preset, ghostResources(state.ghostPreview));
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
  try {
    const exportFullSize = state.preset.pipeline.output.preserveOriginalResolution === true;
    if (!exportFullSize && !state.lastRender?.width) {
      updateStatus("PREVIEW NOT READY");
      return;
    }

    const format = state.preset.pipeline.output.format || "png";
    const mime = exportMime(format);
    const extension = `.${format}`;
    const sizeSuffix = exportFullSize ? "full" : "preview";
    const filename = `${safeFilename(state.preset.name)}-${sizeSuffix}${extension}`;
    const writeBlob = await createBlobWriter(filename, mime, extension);
    if (!writeBlob) {
      updateStatus("EXPORT CANCELED");
      return;
    }
    const exportRequest = { format, mime, writeBlob };

    if (!exportFullSize) {
      updateStatus("EXPORTING · PREVIEW");
      await nextFrame();
      await completeExport(previewImageData(), exportRequest);
      return;
    }

    updateStatus("EXPORTING · FULL RES");
    await nextFrame();

    const size = sourceSize();
    const canvas = document.createElement("canvas");
    canvas.width = size.width;
    canvas.height = size.height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    context.drawImage(state.source, 0, 0, size.width, size.height);
    const imageData = context.getImageData(0, 0, size.width, size.height);
    const preset = effectivePreset();
    const exportGhost = state.ghostSource ? rasterizeGhost(GHOST_EXPORT_MAX_DIMENSION) : null;

    if (worker) {
      exportJobId += 1;
      pendingExportRequest = exportRequest;
      worker.postMessage(
        {
          type: "export",
          jobId: exportJobId,
          width: imageData.width,
          height: imageData.height,
          buffer: imageData.data.buffer,
          preset,
          ghost: ghostPayload(exportGhost)
        },
        [imageData.data.buffer]
      );
    } else {
      processCircuitBendImageData(imageData, preset, ghostResources(exportGhost));
      await completeExport(imageData, exportRequest);
    }
  } catch (error) {
    console.error(error);
    pendingExportRequest = null;
    updateStatus("EXPORT FAILED");
  }
}

function previewImageData() {
  const context = state.lastRender.getContext("2d");
  return context.getImageData(0, 0, state.lastRender.width, state.lastRender.height);
}

function exportMime(format) {
  return `image/${format === "jpg" ? "jpeg" : format}`;
}

async function completeExport(imageData, exportRequest) {
  try {
    pendingExportRequest = null;
    const canvas = document.createElement("canvas");
    canvas.width = imageData.width;
    canvas.height = imageData.height;
    canvas.getContext("2d").putImageData(imageData, 0, 0);
    const request = exportRequest || {
      format: "png",
      mime: "image/png",
      writeBlob: async (blob) => downloadBlob(blob, `${safeFilename(state.preset.name)}.png`)
    };
    const blob = await canvasToBlob(canvas, request.mime, 0.95);
    await request.writeBlob(blob);
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
          preset,
          ghost: ghostPayload(state.ghostThumb)
        },
        [copy.buffer]
      );
    } else {
      const image = {
        width: thumbSource.width,
        height: thumbSource.height,
        data: new Uint8ClampedArray(thumbSource.data)
      };
      processCircuitBendImageData(image, preset, ghostResources(state.ghostThumb));
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
