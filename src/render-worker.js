import { processCircuitBendImageData } from "./engine-core.js";

self.onmessage = (event) => {
  const { type, jobId, index, width, height, buffer, preset } = event.data;
  const data = new Uint8ClampedArray(buffer);
  const image = { width, height, data };
  const startedAt = performance.now();
  processCircuitBendImageData(image, preset);
  const elapsed = Math.round(performance.now() - startedAt);
  self.postMessage({ type, jobId, index, width, height, elapsed, buffer: data.buffer }, [
    data.buffer
  ]);
};
