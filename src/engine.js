import { processCircuitBendImageData } from "./engine-core.js";
import { fitWithin } from "./utils.js";

export function renderCircuitBend(source, preset, options = {}) {
  const sourceWidth = source.width || source.naturalWidth;
  const sourceHeight = source.height || source.naturalHeight;
  const maxDimension = options.maxDimension || null;
  const fitted = fitWithin(sourceWidth, sourceHeight, maxDimension);
  const canvas = document.createElement("canvas");
  canvas.width = fitted.width;
  canvas.height = fitted.height;

  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  context.drawImage(source, 0, 0, canvas.width, canvas.height);

  const image = context.getImageData(0, 0, canvas.width, canvas.height);
  processCircuitBendImageData(image, preset, options.resources || {});
  context.putImageData(image, 0, 0);
  return canvas;
}
