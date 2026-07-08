import { processCircuitBendImageData } from "./engine-core.js";
import {
  ADVANCED_CONTROL_HELP,
  ADVANCED_DEFS,
  ADVANCED_MODULE_HELP,
  applyMacrosToPipeline,
  clonePreset,
  MACRO_DEFS,
  normalizePreset,
  TEMPORAL_MODES
} from "./presets.js";
import { BUILT_IN_PRESETS } from "./built-in-presets.js";
import { RANDOM_FAMILIES, RANDOM_MODES, randomizeModule, randomizePreset } from "./randomize.js";
import { prepareTemporalFrame } from "./temporal.js";
import {
  clamp,
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
const GALLERY_THUMB_MAX_DIMENSION = 300;
const GHOST_EXPORT_MAX_DIMENSION = 3000;
const VIDEO_SHEET_FRAMES = 25;
const VIDEO_THUMB_MAX_DIMENSION = 300;
const HISTORY_LIMIT = 60;
const PREVIEW_MIN_ZOOM = 1;
const PREVIEW_MAX_ZOOM = 8;
const PREVIEW_WHEEL_ZOOM_STEP = 520;

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
  splitPointerId: null,
  previewView: {
    scale: 1,
    panX: 0,
    panY: 0,
    pointerId: null,
    startX: 0,
    startY: 0,
    startPanX: 0,
    startPanY: 0
  },
  soloModule: null,
  frozenModules: new Set(),
  freezeSeed: false,
  openGroups: new Set(),
  moduleHelpKey: null,
  expandedModuleHelp: new Set(),
  activePresetIndex: 0,
  pendingSnapshot: null,
  thumbsPending: false,
  // Video mode: the loaded clip and which of its frames is the working image.
  video: null,
  frameIndex: null,
  workingFrameTime: 0,
  videoSheetSelection: -1,
  videoSheetDirty: true,
  videoGhostPreview: null,
  videoGhostLag: 0
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
  physicsControls: document.querySelector("#physicsControls"),
  physicsAllOffButton: document.querySelector("#physicsAllOffButton"),
  stylizedAllOffButton: document.querySelector("#stylizedAllOffButton"),
  randomFamilyList: document.querySelector("#randomFamilyList"),
  randomMode: document.querySelector("#randomMode"),
  presetName: document.querySelector("#presetName"),
  presetDescription: document.querySelector("#presetDescription"),
  exportFormat: document.querySelector("#exportFormat"),
  exportSize: document.querySelector("#exportSize"),
  dropTarget: document.querySelector("#dropTarget"),
  compareButton: document.querySelector("#compareButton"),
  splitButton: document.querySelector("#splitButton"),
  resetViewButton: document.querySelector("#resetViewButton"),
  seedInput: document.querySelector("#seedInput"),
  rerollButton: document.querySelector("#rerollButton"),
  seedLockButton: document.querySelector("#seedLockButton"),
  helpButton: document.querySelector("#helpButton"),
  helpDialog: document.querySelector("#helpDialog"),
  helpClose: document.querySelector("#helpClose"),
  galleryButton: document.querySelector("#galleryButton"),
  galleryDialog: document.querySelector("#galleryDialog"),
  galleryClose: document.querySelector("#galleryClose"),
  galleryFilter: document.querySelector("#galleryFilter"),
  galleryCount: document.querySelector("#galleryCount"),
  galleryHint: document.querySelector("#galleryHint"),
  galleryGrid: document.querySelector("#galleryGrid"),
  framesButton: document.querySelector("#framesButton"),
  videoDialog: document.querySelector("#videoDialog"),
  videoClose: document.querySelector("#videoClose"),
  videoMeta: document.querySelector("#videoMeta"),
  videoGrid: document.querySelector("#videoGrid"),
  temporalSection: document.querySelector("#temporalSection"),
  temporalControls: document.querySelector("#temporalControls"),
  copyCommandButton: document.querySelector("#copyCommandButton")
};

const thumbCanvases = [];
const galleryCanvases = [];
const videoCanvases = [];
// Gallery thumbnails are expensive (22 renders at 300px), so they are only
// generated when the gallery is opened and the source has changed since.
let galleryThumbsDirty = true;
let worker = null;
let previewJobId = 0;
let exportJobId = 0;
let pendingExportRequest = null;
let thumbSource = null;

init();

function init() {
  worker = setupWorker();
  renderGalleryGrid();
  renderPresetList();
  renderRandomControls();
  renderControls();
  bindEvents();
  applyPreviewView();
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
  } else if (type === "thumb" || type === "gallery-thumb" || type === "video-thumb") {
    const canvas = (
      type === "thumb" ? thumbCanvases : type === "gallery-thumb" ? galleryCanvases : videoCanvases
    )[index];
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
    if (file) loadMediaFile(file);
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
    const file = [...event.dataTransfer.files].find(
      (item) => item.type.startsWith("image/") || isVideoFile(item)
    );
    if (file) loadMediaFile(file);
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

  dom.galleryButton.addEventListener("click", openGallery);
  dom.galleryClose.addEventListener("click", () => dom.galleryDialog.close());
  dom.galleryDialog.addEventListener("click", (event) => {
    if (event.target === dom.galleryDialog) dom.galleryDialog.close();
  });
  dom.framesButton.addEventListener("click", openVideoSheet);
  dom.videoClose.addEventListener("click", () => dom.videoDialog.close());
  dom.videoDialog.addEventListener("click", (event) => {
    if (event.target === dom.videoDialog) dom.videoDialog.close();
  });
  dom.copyCommandButton.addEventListener("click", copyRenderCommand);
  dom.physicsAllOffButton.addEventListener("click", (event) => panelAllOff(event, true, "PHYSICS RAIL OFF"));
  dom.stylizedAllOffButton.addEventListener("click", (event) => panelAllOff(event, false, "STYLIZED CIRCUIT OFF"));

  dom.galleryFilter.addEventListener("input", applyGalleryFilter);
  dom.galleryFilter.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown" || event.key === "Enter") {
      const first = visibleGalleryCards()[0];
      if (first) {
        event.preventDefault();
        first.focus();
      }
    }
  });
  dom.galleryGrid.addEventListener("keydown", handleGalleryGridKeys);

  dom.compareButton.addEventListener("pointerdown", () => setComparing(true));
  dom.compareButton.addEventListener("pointerup", () => setComparing(false));
  dom.compareButton.addEventListener("pointerleave", () => setComparing(false));
  dom.splitButton.addEventListener("click", toggleSplitMode);
  dom.resetViewButton.addEventListener("click", () => resetPreviewView(true));

  dom.dropTarget.addEventListener("wheel", handlePreviewWheel, { passive: false });
  dom.previewCanvas.addEventListener("dblclick", () => resetPreviewView(true));
  dom.previewCanvas.addEventListener("pointerdown", handlePreviewPointerDown);
  dom.previewCanvas.addEventListener("pointermove", handlePreviewPointerMove);
  dom.previewCanvas.addEventListener("pointerup", endPreviewPointerInteraction);
  dom.previewCanvas.addEventListener("pointercancel", endPreviewPointerInteraction);
  dom.previewCanvas.addEventListener("lostpointercapture", endPreviewPointerInteraction);
  window.addEventListener("resize", () => {
    clampPreviewView();
    applyPreviewView();
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
    if (event.key === "g" || event.key === "G") {
      // Without preventDefault the same keystroke types "g" into the
      // freshly focused gallery filter input.
      event.preventDefault();
      if (dom.galleryDialog.open) dom.galleryDialog.close();
      else if (!dom.helpDialog.open) openGallery();
      return;
    }
    if ((event.key === "v" || event.key === "V") && state.video) {
      event.preventDefault();
      if (dom.videoDialog.open) dom.videoDialog.close();
      else if (!dom.helpDialog.open && !dom.galleryDialog.open) openVideoSheet();
      return;
    }
    // While the gallery or frames sheet is up, only its own keys apply —
    // the editor shortcuts below would act on the app hidden behind the modal.
    if (dom.galleryDialog.open || dom.videoDialog.open) return;
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
  state.splitPointerId = null;
  dom.splitButton.classList.toggle("is-active", state.splitMode);
  dom.dropTarget.classList.toggle("is-split", state.splitMode);
  drawComposite();
  updateStatus(state.splitMode ? "SPLIT COMPARE · DRAG IMAGE" : "SPLIT OFF");
}

function updateSplitFromPointer(event) {
  const rect = dom.previewCanvas.getBoundingClientRect();
  if (rect.width <= 0) return;
  state.splitRatio = clamp((event.clientX - rect.left) / rect.width);
  drawComposite();
}

function handlePreviewWheel(event) {
  if (!state.source || !state.lastRender) return;
  if (event.target.closest?.(".viewer-toolbar")) return;
  event.preventDefault();

  const deltaScale =
    event.deltaMode === 1
      ? 16
      : event.deltaMode === 2
        ? Math.max(1, dom.dropTarget.clientHeight)
        : 1;
  const deltaY = event.deltaY * deltaScale;
  if (!Number.isFinite(deltaY) || deltaY === 0) return;

  const nextScale = clamp(
    state.previewView.scale * 2 ** (-deltaY / PREVIEW_WHEEL_ZOOM_STEP),
    PREVIEW_MIN_ZOOM,
    PREVIEW_MAX_ZOOM
  );
  zoomPreviewAt(event.clientX, event.clientY, nextScale);
}

function handlePreviewPointerDown(event) {
  if (!state.source || !state.lastRender || event.button !== 0) return;

  if (state.splitMode) {
    event.preventDefault();
    state.splitPointerId = event.pointerId;
    dom.previewCanvas.setPointerCapture(event.pointerId);
    updateSplitFromPointer(event);
    return;
  }

  if (state.previewView.scale <= PREVIEW_MIN_ZOOM + 0.001) return;
  event.preventDefault();
  const view = state.previewView;
  view.pointerId = event.pointerId;
  view.startX = event.clientX;
  view.startY = event.clientY;
  view.startPanX = view.panX;
  view.startPanY = view.panY;
  dom.previewCanvas.setPointerCapture(event.pointerId);
  applyPreviewView();
}

function handlePreviewPointerMove(event) {
  if (state.splitPointerId === event.pointerId) {
    if (event.buttons === 0) {
      endPreviewPointerInteraction(event);
      return;
    }
    event.preventDefault();
    updateSplitFromPointer(event);
    return;
  }

  const view = state.previewView;
  if (view.pointerId !== event.pointerId) return;
  if (event.buttons === 0) {
    endPreviewPointerInteraction(event);
    return;
  }
  event.preventDefault();
  view.panX = view.startPanX + event.clientX - view.startX;
  view.panY = view.startPanY + event.clientY - view.startY;
  clampPreviewView();
  applyPreviewView();
}

function endPreviewPointerInteraction(event) {
  if (state.splitPointerId === event.pointerId) state.splitPointerId = null;

  if (state.previewView.pointerId === event.pointerId) {
    state.previewView.pointerId = null;
    applyPreviewView();
  }

  if (dom.previewCanvas.hasPointerCapture?.(event.pointerId)) {
    dom.previewCanvas.releasePointerCapture(event.pointerId);
  }
}

function zoomPreviewAt(clientX, clientY, nextScale) {
  const view = state.previewView;
  const previousScale = view.scale;
  if (!Number.isFinite(nextScale) || Math.abs(nextScale - previousScale) < 0.001) return;

  const baseRect = previewBaseRect();
  if (baseRect.width <= 0 || baseRect.height <= 0) return;
  const anchor = previewZoomAnchor(clientX, clientY);
  const imageX = (anchor.x - baseRect.left - view.panX) / previousScale;
  const imageY = (anchor.y - baseRect.top - view.panY) / previousScale;

  view.scale = nextScale;
  view.panX = anchor.x - baseRect.left - imageX * nextScale;
  view.panY = anchor.y - baseRect.top - imageY * nextScale;
  clampPreviewView();
  applyPreviewView();
  updateStatus(view.scale > PREVIEW_MIN_ZOOM + 0.001 ? `VIEW ${Math.round(view.scale * 100)}%` : "VIEW FIT");
}

function previewZoomAnchor(clientX, clientY) {
  const canvasRect = dom.previewCanvas.getBoundingClientRect();
  const isOverCanvas =
    clientX >= canvasRect.left &&
    clientX <= canvasRect.right &&
    clientY >= canvasRect.top &&
    clientY <= canvasRect.bottom;
  if (isOverCanvas) return { x: clientX, y: clientY };

  const viewerRect = dom.dropTarget.getBoundingClientRect();
  return {
    x: viewerRect.left + viewerRect.width / 2,
    y: viewerRect.top + viewerRect.height / 2
  };
}

function previewBaseRect() {
  const rect = dom.previewCanvas.getBoundingClientRect();
  const view = state.previewView;
  return {
    left: rect.left - view.panX,
    top: rect.top - view.panY,
    width: dom.previewCanvas.offsetWidth || rect.width / Math.max(view.scale, 0.001),
    height: dom.previewCanvas.offsetHeight || rect.height / Math.max(view.scale, 0.001)
  };
}

function previewViewportRect() {
  const rect = dom.dropTarget.getBoundingClientRect();
  const style = getComputedStyle(dom.dropTarget);
  const left = rect.left + Number.parseFloat(style.paddingLeft || 0);
  const right = rect.right - Number.parseFloat(style.paddingRight || 0);
  const top = rect.top + Number.parseFloat(style.paddingTop || 0);
  const bottom = rect.bottom - Number.parseFloat(style.paddingBottom || 0);
  return {
    left,
    right,
    top,
    bottom,
    width: Math.max(0, right - left),
    height: Math.max(0, bottom - top)
  };
}

function clampPreviewView() {
  const view = state.previewView;
  view.scale = clamp(view.scale || PREVIEW_MIN_ZOOM, PREVIEW_MIN_ZOOM, PREVIEW_MAX_ZOOM);

  if (!state.source || !state.lastRender || view.scale <= PREVIEW_MIN_ZOOM + 0.001) {
    view.scale = PREVIEW_MIN_ZOOM;
    view.panX = 0;
    view.panY = 0;
    return;
  }

  const baseRect = previewBaseRect();
  const viewportRect = previewViewportRect();
  const scaledWidth = baseRect.width * view.scale;
  const scaledHeight = baseRect.height * view.scale;

  if (scaledWidth <= viewportRect.width) {
    view.panX = 0;
  } else {
    view.panX = clamp(
      view.panX,
      viewportRect.right - baseRect.left - scaledWidth,
      viewportRect.left - baseRect.left
    );
  }

  if (scaledHeight <= viewportRect.height) {
    view.panY = 0;
  } else {
    view.panY = clamp(
      view.panY,
      viewportRect.bottom - baseRect.top - scaledHeight,
      viewportRect.top - baseRect.top
    );
  }
}

function resetPreviewView(showStatus = false) {
  const view = state.previewView;
  view.scale = PREVIEW_MIN_ZOOM;
  view.panX = 0;
  view.panY = 0;
  view.pointerId = null;
  state.splitPointerId = null;
  applyPreviewView();
  if (showStatus && state.source) updateStatus("VIEW FIT");
}

function applyPreviewView() {
  const view = state.previewView;
  const isZoomed = state.source && view.scale > PREVIEW_MIN_ZOOM + 0.001;
  dom.previewCanvas.style.transform = isZoomed
    ? `matrix(${roundTransform(view.scale)}, 0, 0, ${roundTransform(view.scale)}, ${roundTransform(view.panX)}, ${roundTransform(view.panY)})`
    : "";
  dom.dropTarget.classList.toggle("is-zoomed", Boolean(isZoomed));
  dom.dropTarget.classList.toggle("is-panning", state.previewView.pointerId !== null);
  dom.resetViewButton.disabled = !state.source;
  dom.resetViewButton.textContent = isZoomed ? `${Math.round(view.scale * 100)}%` : "Fit";
  dom.resetViewButton.title = isZoomed
    ? "Reset preview zoom (double-click image)"
    : "Preview fit to window";
}

function roundTransform(value) {
  return Math.round(value * 1000) / 1000;
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
  clampPreviewView();
  applyPreviewView();
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
  dom.galleryGrid.querySelectorAll(".gallery-card").forEach((card, i) => {
    card.classList.toggle("is-active", i === index);
  });
}

function applyBuiltInPreset(index) {
  pushHistory(snapshotPreset());
  state.preset = clonePreset(BUILT_IN_PRESETS[index]);
  setActivePreset(index);
  renderControls();
  scheduleRender();
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
    button.addEventListener("click", () => applyBuiltInPreset(index));
    thumbCanvases.push(thumb);
    dom.presetList.append(button);
  });
  setActivePreset(state.activePresetIndex);
}

// --- Camera gallery ---

function renderGalleryGrid() {
  dom.galleryGrid.innerHTML = "";
  galleryCanvases.length = 0;
  BUILT_IN_PRESETS.forEach((preset, index) => {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "gallery-card";
    card.dataset.search = `${preset.name} ${preset.tags.join(" ")} ${preset.description}`.toLowerCase();
    const thumb = document.createElement("canvas");
    thumb.className = "gallery-thumb";
    thumb.width = 4;
    thumb.height = 3;
    const name = document.createElement("span");
    name.className = "gallery-card-name";
    name.textContent = preset.name;
    const tags = document.createElement("small");
    tags.className = "gallery-card-tags";
    tags.textContent = preset.tags.join(" / ");
    const description = document.createElement("small");
    description.className = "gallery-card-desc";
    description.textContent = preset.description;
    card.append(thumb, name, tags, description);
    card.addEventListener("click", () => {
      applyBuiltInPreset(index);
      dom.galleryDialog.close();
      updateStatus(`${preset.name.toUpperCase()} LOADED`);
    });
    galleryCanvases.push(thumb);
    dom.galleryGrid.append(card);
  });
}

function openGallery() {
  if (state.source && galleryThumbsDirty) {
    galleryThumbsDirty = false;
    generateGalleryThumbnails();
  }
  dom.galleryHint.hidden = !!state.source;
  dom.galleryFilter.value = "";
  applyGalleryFilter();
  dom.galleryDialog.showModal();
  const active = dom.galleryGrid.querySelector(".gallery-card.is-active");
  (active || dom.galleryFilter).focus();
}

function applyGalleryFilter() {
  const query = dom.galleryFilter.value.trim().toLowerCase();
  let shown = 0;
  dom.galleryGrid.querySelectorAll(".gallery-card").forEach((card) => {
    const match = !query || card.dataset.search.includes(query);
    card.hidden = !match;
    if (match) shown += 1;
  });
  dom.galleryCount.textContent = `${shown}/${BUILT_IN_PRESETS.length}`;
}

function visibleGalleryCards() {
  return [...dom.galleryGrid.querySelectorAll(".gallery-card:not([hidden])")];
}

function handleGalleryGridKeys(event) {
  const keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
  if (!keys.includes(event.key)) return;
  const cards = visibleGalleryCards();
  const current = cards.indexOf(document.activeElement);
  if (current === -1) return;
  // Column count from layout: cards sharing the first card's row.
  const firstTop = cards[0].offsetTop;
  let columns = cards.findIndex((card) => card.offsetTop !== firstTop);
  if (columns === -1) columns = cards.length;
  const step = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : event.key === "ArrowUp" ? -columns : columns;
  const next = current + step;
  if (event.key === "ArrowUp" && next < 0) {
    event.preventDefault();
    dom.galleryFilter.focus();
    return;
  }
  if (next < 0 || next >= cards.length) return;
  event.preventDefault();
  cards[next].focus();
  cards[next].scrollIntoView({ block: "nearest" });
}

function generateGalleryThumbnails() {
  const size = sourceSize();
  const fitted = fitWithin(size.width, size.height, GALLERY_THUMB_MAX_DIMENSION);
  const canvas = document.createElement("canvas");
  canvas.width = fitted.width;
  canvas.height = fitted.height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  context.drawImage(state.source, 0, 0, fitted.width, fitted.height);
  const source = context.getImageData(0, 0, fitted.width, fitted.height);
  renderThumbSet(source, "gallery-thumb", galleryCanvases);
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
  // Module groups live in either circuit panel (physics rail or stylized).
  const button = document.querySelector(`.advanced-group[data-module-key="${moduleKey}"] .lock-button`);
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
  renderTemporalControls();
  renderAdvancedControls();
}

const TEMPORAL_DEFS = [
  ["temporal.mode", "Seed Mode", "select", TEMPORAL_MODES, null, null,
    "How structural damage evolves across frames: locked = frozen in place, hold = re-rolls every ~N frames, flicker = re-rolls every frame. Sensor grain always shimmers regardless."],
  ["temporal.holdFrames", "Hold Frames", "range", 2, 60, 1,
    "Average frames between damage re-rolls in hold mode; each hold varies ±40% so the stutter is irregular."],
  ["temporal.driftAmount", "Drift Amount", "range", 0, 1, 0.01,
    "How far module parameters wander from their set values over the clip."],
  ["temporal.driftSpeed", "Drift Speed", "range", 0, 1, 0.01,
    "How fast the parameter wander evolves."],
  ["temporal.ghostFrame", "Ghost Lag", "range", 0, 30, 1,
    "Feeds the Buffer Ghost module the input frame N back (0 = off). Enable Buffer Ghost to see the trails."]
];

function renderTemporalControls() {
  dom.temporalSection.hidden = !state.video;
  if (!state.video) return;
  dom.temporalControls.innerHTML = "";

  TEMPORAL_DEFS.forEach(([path, label, type, min, max, step, tooltip]) => {
    const value = getAtPath(state.preset, path);
    const row = document.createElement("label");
    row.title = tooltip;

    if (type === "select") {
      row.className = "control-row is-select";
      row.innerHTML = `
        <span>${label}</span>
        <select>
          ${min
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

    dom.temporalControls.append(row);
  });
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

// ALL OFF in a circuit panel header: switch off every module in that panel
// (physics rail or stylized) without touching their other settings.
function panelAllOff(event, physics, statusLabel) {
  // The button lives inside a <summary>; don't toggle the dropdown with it.
  event.preventDefault();
  event.stopPropagation();
  pushHistory(snapshotPreset());
  ADVANCED_DEFS.forEach((group) => {
    if (Boolean(group.physics) !== physics) return;
    setAtPath(state.preset, `pipeline.${group.key}.enabled`, false);
  });
  renderAdvancedControls();
  scheduleRender();
  updateStatus(statusLabel);
}

function createModuleHelpPanel(group, help, expanded) {
  const panel = document.createElement("div");
  panel.className = "module-help";
  panel.id = `module-help-${group.key}`;

  const shortText = document.createElement("p");
  shortText.className = "module-help-short";
  shortText.textContent = help.short;
  panel.append(shortText);

  if (help.long) {
    const longText = document.createElement("p");
    longText.className = "module-help-long";
    longText.hidden = !expanded;
    longText.textContent = help.long;

    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "module-help-toggle";
    toggle.textContent = expanded ? "Less" : "More";
    toggle.setAttribute("aria-expanded", String(expanded));
    toggle.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      if (expanded) state.expandedModuleHelp.delete(group.key);
      else state.expandedModuleHelp.add(group.key);
      renderAdvancedControls();
    });

    panel.append(longText, toggle);
  }

  return panel;
}

function renderAdvancedControls() {
  dom.advancedControls.innerHTML = "";
  dom.physicsControls.innerHTML = "";
  ADVANCED_DEFS.forEach((group) => {
    const moduleFrozen = state.frozenModules.has(group.key);
    const moduleHelp = ADVANCED_MODULE_HELP[group.key];
    const helpOpen = Boolean(moduleHelp) && state.moduleHelpKey === group.key;
    const helpExpanded = state.expandedModuleHelp.has(group.key);
    const details = document.createElement("details");
    details.className = "advanced-group";
    details.dataset.moduleKey = group.key;
    details.classList.toggle("is-frozen", moduleFrozen);
    if (state.soloModule) {
      details.classList.toggle("is-solo", state.soloModule === group.key);
      details.classList.toggle("is-dimmed", state.soloModule !== group.key);
    }
    details.open = state.openGroups.has(group.group) || helpOpen;
    details.addEventListener("toggle", () => {
      if (details.open) state.openGroups.add(group.group);
      else {
        state.openGroups.delete(group.group);
        if (state.moduleHelpKey === group.key) state.moduleHelpKey = null;
      }
    });

    const summary = document.createElement("summary");
    // Every pipeline module has an `enabled` flag; the lamp is its only UI
    // (there is no "Enabled" row inside the group).
    const enabledPath = `pipeline.${group.key}.enabled`;
    const lampHelp = ADVANCED_CONTROL_HELP[enabledPath] || "Toggle module on/off";
    summary.innerHTML = `
      <span>${group.group}</span>
      <span class="summary-tools">
        <button type="button" class="lock-button" title="${moduleFrozen ? "Module frozen during randomize" : "Freeze this module during randomize"}" aria-label="${moduleFrozen ? "Unfreeze" : "Freeze"} ${group.group}" aria-pressed="${moduleFrozen}">${lockIconSvg(moduleFrozen)}</button>
        <button type="button" class="dice-button" title="Randomize this module (uses Randomize mode)" aria-label="Randomize ${group.group}">${diceIconSvg()}</button>
        <button type="button" class="solo-button" title="Solo this module">S</button>
        <button type="button" class="info-button ${helpOpen ? "is-active" : ""}" title="Explain ${group.group}" aria-label="Explain ${group.group}" aria-expanded="${helpOpen}" aria-controls="module-help-${group.key}">i</button>
        <button type="button" class="group-lamp" title="${lampHelp}"></button>
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

    const infoButton = summary.querySelector(".info-button");
    if (infoButton) {
      infoButton.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        if (helpOpen) {
          state.moduleHelpKey = null;
        } else {
          state.moduleHelpKey = group.key;
          state.openGroups.add(group.group);
        }
        renderAdvancedControls();
      });
    }

    if (moduleHelp && helpOpen) {
      details.append(createModuleHelpPanel(group, moduleHelp, helpExpanded));
    }

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
      } else if (type === "bits") {
        // DIP-switch bank for busBend bit masks: `min` carries the list of
        // ADC bit indices this bank can reach, high bit first.
        row.className = "control-row is-bits";
        const bankBits = min;
        row.innerHTML = `
          <span>${label}</span>
          <span class="bit-bank">
            ${bankBits
              .map(
                (bit) =>
                  `<button type="button" class="bit-switch ${(value | 0) & (1 << bit) ? "is-on" : ""}" data-bit="${bit}" title="D${bit}"><i></i><em>${bit}</em></button>`
              )
              .join("")}
          </span>
        `;
        row.querySelectorAll(".bit-switch").forEach((button) => {
          button.addEventListener("click", (event) => {
            event.preventDefault();
            pushHistory(snapshotPreset());
            const bit = Number(button.dataset.bit);
            const next = ((getAtPath(state.preset, path) | 0) ^ (1 << bit)) >>> 0;
            setAtPath(state.preset, path, next);
            button.classList.toggle("is-on", Boolean(next & (1 << bit)));
            scheduleRender();
          });
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

    (group.physics ? dom.physicsControls : dom.advancedControls).append(details);
  });
}

// --- Image / preset IO ---

function isVideoFile(file) {
  return file.type.startsWith("video/") || /\.(mp4|mov|m4v|webm|mkv|avi)$/i.test(file.name);
}

function loadMediaFile(file) {
  if (isVideoFile(file)) loadVideoFile(file);
  else loadImageFile(file);
}

async function loadImageFile(file) {
  updateStatus("LOADING IMAGE");
  try {
    state.source = await fileToImageSource(file);
    disposeVideo();
    state.sourceName = file.name;
    dom.dimensionsText.textContent = `${state.source.width} x ${state.source.height}`;
    dom.emptyState.hidden = true;
    resetPreviewView();
    prepareThumbSource();
    state.thumbsPending = true;
    galleryThumbsDirty = true;
    renderTemporalControls();
    scheduleRender();
  } catch (error) {
    console.error(error);
    updateStatus("IMAGE LOAD FAILED");
  }
}

// --- Video mode ---
// A loaded video is a frame source: the contact sheet shows evenly spaced
// frames through the current camera, and clicking one makes it the working
// image (carrying its frame index, so the preview matches what the CLI
// renders for that frame). The app never encodes video — that's the CLI's
// job via the Copy Render Command button.

async function loadVideoFile(file) {
  updateStatus("LOADING VIDEO");
  try {
    disposeVideo();
    const url = URL.createObjectURL(file);
    const element = document.createElement("video");
    element.muted = true;
    element.playsInline = true;
    element.preload = "auto";
    element.src = url;
    await new Promise((resolve, reject) => {
      element.onloadeddata = resolve;
      element.onerror = () => reject(new Error(`Cannot decode video: ${file.name}`));
    });
    const fps = await estimateVideoFps(element);
    const duration = element.duration;
    if (!Number.isFinite(duration) || duration <= 0 || !element.videoWidth) {
      throw new Error("Video has no readable duration or dimensions");
    }

    const keyframes = [];
    for (let i = 0; i < VIDEO_SHEET_FRAMES; i += 1) {
      const time = (duration * (i + 0.5)) / VIDEO_SHEET_FRAMES;
      keyframes.push({
        time,
        index: Math.round(time * fps),
        thumb: null,
        ghostThumb: null,
        ghostThumbLag: 0
      });
    }

    state.video = {
      element,
      url,
      name: file.name,
      duration,
      fps,
      width: element.videoWidth,
      height: element.videoHeight,
      keyframes
    };
    state.videoSheetDirty = true;
    dom.framesButton.hidden = false;
    renderTemporalControls();
    renderVideoGrid();
    await selectVideoFrame(Math.floor(VIDEO_SHEET_FRAMES / 2));
    openVideoSheet();
  } catch (error) {
    console.error(error);
    updateStatus("VIDEO LOAD FAILED");
  }
}

function disposeVideo() {
  if (!state.video) return;
  const { element, url } = state.video;
  element.removeAttribute("src");
  element.load();
  URL.revokeObjectURL(url);
  state.video = null;
  state.frameIndex = null;
  state.workingFrameTime = 0;
  state.videoSheetSelection = -1;
  state.videoGhostPreview = null;
  state.videoGhostLag = 0;
  dom.framesButton.hidden = true;
  if (dom.videoDialog.open) dom.videoDialog.close();
  videoCanvases.length = 0;
  dom.videoGrid.innerHTML = "";
}

// HTMLVideoElement does not expose the frame rate, but temporal seeds are
// scheduled in frames. Measure it from mediaTime deltas during a short muted
// play; the CLI uses the container's true rate, so a wrong estimate here only
// shifts which preview frames land on which hold segments.
async function estimateVideoFps(element) {
  if (!("requestVideoFrameCallback" in HTMLVideoElement.prototype)) return 30;
  try {
    const times = [];
    await element.play();
    await new Promise((resolve) => {
      const stop = window.setTimeout(resolve, 1200);
      const tick = (_now, meta) => {
        times.push(meta.mediaTime);
        if (times.length >= 12) {
          window.clearTimeout(stop);
          resolve();
        } else {
          element.requestVideoFrameCallback(tick);
        }
      };
      element.requestVideoFrameCallback(tick);
    });
    element.pause();
    const deltas = [];
    for (let i = 1; i < times.length; i += 1) {
      const delta = times[i] - times[i - 1];
      if (delta > 0.0005) deltas.push(delta);
    }
    if (deltas.length < 4) return 30;
    deltas.sort((a, b) => a - b);
    const fps = 1 / deltas[Math.floor(deltas.length / 2)];
    const common = [23.976, 24, 25, 29.97, 30, 48, 50, 59.94, 60, 120];
    const snapped = common.reduce((best, rate) =>
      Math.abs(rate - fps) < Math.abs(best - fps) ? rate : best
    );
    return Math.abs(snapped - fps) / fps < 0.05 ? snapped : Math.round(fps * 100) / 100;
  } catch (error) {
    console.warn("FPS estimation failed, assuming 30", error);
    return 30;
  }
}

// All seeks share one <video> element, so they must run one at a time.
let videoSeekChain = Promise.resolve();

function withVideoElement(task) {
  const run = videoSeekChain.then(task, task);
  videoSeekChain = run.then(
    () => {},
    () => {}
  );
  return run;
}

function seekVideoTo(element, time) {
  const target = clamp(time, 0, Math.max(0, element.duration - 0.001));
  if (Math.abs(element.currentTime - target) < 0.002 && element.readyState >= 2) {
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    const onSeeked = () => {
      element.removeEventListener("error", onError);
      resolve();
    };
    const onError = () => {
      element.removeEventListener("seeked", onSeeked);
      reject(new Error("Video seek failed"));
    };
    element.addEventListener("seeked", onSeeked, { once: true });
    element.addEventListener("error", onError, { once: true });
    element.currentTime = target;
  });
}

function grabVideoCanvas(time, maxDimension) {
  return withVideoElement(async () => {
    const element = state.video.element;
    await seekVideoTo(element, time);
    const fitted = fitWithin(element.videoWidth, element.videoHeight, maxDimension);
    const canvas = document.createElement("canvas");
    canvas.width = fitted.width;
    canvas.height = fitted.height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    context.imageSmoothingEnabled = true;
    context.imageSmoothingQuality = "high";
    context.drawImage(element, 0, 0, fitted.width, fitted.height);
    return canvas;
  });
}

async function grabVideoImageData(time, maxDimension) {
  const canvas = await grabVideoCanvas(time, maxDimension);
  return canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height);
}

async function selectVideoFrame(sheetIndex) {
  const video = state.video;
  const keyframe = video.keyframes[sheetIndex];
  updateStatus("GRABBING FRAME");
  const canvas = await grabVideoCanvas(keyframe.time, null);
  state.source = canvas;
  state.sourceName = video.name;
  state.frameIndex = keyframe.index;
  state.workingFrameTime = keyframe.time;
  state.videoSheetSelection = sheetIndex;
  state.videoGhostLag = -1;
  dom.dimensionsText.textContent = `${canvas.width} x ${canvas.height}`;
  dom.emptyState.hidden = true;
  resetPreviewView();
  prepareThumbSource();
  state.thumbsPending = true;
  galleryThumbsDirty = true;
  markVideoSelection();
  scheduleRender();
  updateStatus(`FRAME ${keyframe.index} · ${formatTimecode(keyframe.time)}`);
}

function presetGhostLag() {
  return Math.max(0, Math.round(state.preset.temporal?.ghostFrame || 0));
}

function videoGhostActive() {
  return Boolean(state.video) && state.frameIndex !== null && presetGhostLag() > 0;
}

// The preview's ghost companion (the input frame ghostFrame back) is grabbed
// asynchronously; renders in between use the previous one and a re-render is
// queued once the fresh grab lands.
let videoGhostRefreshing = false;

async function refreshVideoGhostPreview() {
  const lag = videoGhostActive() ? presetGhostLag() : 0;
  state.videoGhostLag = lag;
  if (!lag) {
    state.videoGhostPreview = null;
    return;
  }
  const time = Math.max(0, state.workingFrameTime - lag / state.video.fps);
  state.videoGhostPreview = await grabVideoImageData(time, PREVIEW_MAX_DIMENSION);
}

function ensureVideoGhostFresh() {
  const lag = videoGhostActive() ? presetGhostLag() : 0;
  if (lag === state.videoGhostLag || videoGhostRefreshing) return;
  videoGhostRefreshing = true;
  refreshVideoGhostPreview()
    .catch((error) => console.error(error))
    .finally(() => {
      videoGhostRefreshing = false;
      scheduleRender();
    });
}

function formatTimecode(seconds) {
  const minutes = Math.floor(seconds / 60);
  const rest = seconds - minutes * 60;
  return `${minutes}:${rest.toFixed(1).padStart(4, "0")}`;
}

function renderVideoGrid() {
  dom.videoGrid.innerHTML = "";
  videoCanvases.length = 0;
  state.video.keyframes.forEach((keyframe, index) => {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "gallery-card video-card";
    const thumb = document.createElement("canvas");
    thumb.className = "gallery-thumb";
    thumb.width = 4;
    thumb.height = 3;
    const label = document.createElement("span");
    label.className = "video-card-label";
    label.textContent = `${formatTimecode(keyframe.time)} · f${keyframe.index}`;
    card.append(thumb, label);
    card.addEventListener("click", () => {
      selectVideoFrame(index);
      dom.videoDialog.close();
    });
    videoCanvases.push(thumb);
    dom.videoGrid.append(card);
  });
  markVideoSelection();
}

function markVideoSelection() {
  dom.videoGrid.querySelectorAll(".video-card").forEach((card, index) => {
    card.classList.toggle("is-active", index === state.videoSheetSelection);
  });
}

function openVideoSheet() {
  if (!state.video) return;
  const video = state.video;
  dom.videoMeta.textContent = `${video.name} · ${video.duration.toFixed(1)}s · ${video.fps} fps · ${video.width}x${video.height}`;
  dom.videoDialog.showModal();
  if (state.videoSheetDirty) {
    state.videoSheetDirty = false;
    refreshVideoSheet();
  }
}

// Sheet renders are superseded, not queued: reopening after edits bumps the
// token so stale extraction loops stop early.
let videoSheetToken = 0;

async function refreshVideoSheet() {
  const video = state.video;
  if (!video) return;
  const token = ++videoSheetToken;
  const basePreset = effectivePreset();
  const lag = presetGhostLag();
  updateStatus("RENDERING FRAMES");

  try {
    for (const keyframe of video.keyframes) {
      if (token !== videoSheetToken || state.video !== video) return;
      if (!keyframe.thumb) {
        keyframe.thumb = await grabVideoImageData(keyframe.time, VIDEO_THUMB_MAX_DIMENSION);
      }
      if (lag > 0 && (!keyframe.ghostThumb || keyframe.ghostThumbLag !== lag)) {
        keyframe.ghostThumb = await grabVideoImageData(
          Math.max(0, keyframe.time - lag / video.fps),
          VIDEO_THUMB_MAX_DIMENSION
        );
        keyframe.ghostThumbLag = lag;
      }
    }
  } catch (error) {
    console.error(error);
    updateStatus("FRAME GRAB FAILED");
    return;
  }
  if (token !== videoSheetToken || state.video !== video) return;

  video.keyframes.forEach((keyframe, index) => {
    const frameState = prepareTemporalFrame(basePreset, keyframe.index);
    const ghost = lag > 0 ? keyframe.ghostThumb : state.ghostThumb;
    if (worker) {
      const copy = new Uint8ClampedArray(keyframe.thumb.data);
      worker.postMessage(
        {
          type: "video-thumb",
          index,
          width: keyframe.thumb.width,
          height: keyframe.thumb.height,
          buffer: copy.buffer,
          preset: frameState.preset,
          liveSeed: frameState.liveSeed,
          ghost: ghostPayload(ghost)
        },
        [copy.buffer]
      );
    } else {
      const image = {
        width: keyframe.thumb.width,
        height: keyframe.thumb.height,
        data: new Uint8ClampedArray(keyframe.thumb.data)
      };
      processCircuitBendImageData(image, frameState.preset, ghostResources(ghost), {
        liveSeed: frameState.liveSeed
      });
      const canvas = videoCanvases[index];
      if (!canvas) return;
      canvas.width = image.width;
      canvas.height = image.height;
      canvas
        .getContext("2d")
        .putImageData(new ImageData(image.data, image.width, image.height), 0, 0);
      canvas.classList.add("has-image");
    }
  });
  updateStatus(`${state.preset.name.toUpperCase()} · FRAME SHEET`);
}

function buildCliCommand() {
  const input = state.video?.name || "input.mp4";
  const output = `${input.replace(/\.[^.]+$/, "")}-bent.mp4`;
  const presetFile = `${safeFilename(state.preset.name)}.vcb-preset.json`;
  return `node src/cli.js render-video "${input}" "${output}" --preset "${presetFile}"`;
}

async function copyRenderCommand() {
  const command = buildCliCommand();
  try {
    await navigator.clipboard.writeText(command);
    updateStatus("COMMAND COPIED · SAVE PRESET NEXT TO THE VIDEO");
  } catch {
    window.prompt("Render command", command);
  }
}

async function loadGhostFile(file) {
  updateStatus("LOADING GHOST");
  try {
    state.ghostSource = await fileToImageSource(file);
    state.ghostName = file.name;
    state.ghostPreview = rasterizeGhost(PREVIEW_MAX_DIMENSION);
    state.ghostThumb = rasterizeGhost(THUMB_MAX_DIMENSION);
    galleryThumbsDirty = true;
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
  galleryThumbsDirty = true;
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

function diceIconSvg() {
  return `
    <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <circle cx="8.5" cy="8.5" r="1.25" />
      <circle cx="15.5" cy="8.5" r="1.25" />
      <circle cx="12" cy="12" r="1.25" />
      <circle cx="8.5" cy="15.5" r="1.25" />
      <circle cx="15.5" cy="15.5" r="1.25" />
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
  // Any change that re-renders the preview also stales the frame sheet;
  // it regenerates lazily the next time the sheet is opened.
  state.videoSheetDirty = true;
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
    const frameState = prepareTemporalFrame(effectivePreset(), state.frameIndex ?? NaN);
    const preset = frameState.preset;
    ensureVideoGhostFresh();
    const previewGhost = videoGhostActive() ? state.videoGhostPreview : state.ghostPreview;

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
          liveSeed: frameState.liveSeed,
          ghost: ghostPayload(previewGhost)
        },
        [imageData.data.buffer]
      );
    } else {
      const startedAt = performance.now();
      processCircuitBendImageData(imageData, preset, ghostResources(previewGhost), {
        liveSeed: frameState.liveSeed
      });
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
    const frameState = prepareTemporalFrame(effectivePreset(), state.frameIndex ?? NaN);
    const preset = frameState.preset;
    let exportGhost = state.ghostSource ? rasterizeGhost(GHOST_EXPORT_MAX_DIMENSION) : null;
    if (videoGhostActive()) {
      const ghostTime = Math.max(0, state.workingFrameTime - presetGhostLag() / state.video.fps);
      exportGhost = await grabVideoImageData(ghostTime, null);
    }

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
          liveSeed: frameState.liveSeed,
          ghost: ghostPayload(exportGhost)
        },
        [imageData.data.buffer]
      );
    } else {
      processCircuitBendImageData(imageData, preset, ghostResources(exportGhost), {
        liveSeed: frameState.liveSeed
      });
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
  renderThumbSet(thumbSource, "thumb", thumbCanvases);
}

function renderThumbSet(source, type, canvases) {
  BUILT_IN_PRESETS.forEach((preset, index) => {
    if (worker) {
      const copy = new Uint8ClampedArray(source.data);
      worker.postMessage(
        {
          type,
          index,
          width: source.width,
          height: source.height,
          buffer: copy.buffer,
          preset,
          ghost: ghostPayload(state.ghostThumb)
        },
        [copy.buffer]
      );
    } else {
      const image = {
        width: source.width,
        height: source.height,
        data: new Uint8ClampedArray(source.data)
      };
      processCircuitBendImageData(image, preset, ghostResources(state.ghostThumb));
      const canvas = canvases[index];
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
