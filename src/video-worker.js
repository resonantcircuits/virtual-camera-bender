import { parentPort } from "node:worker_threads";
import { processCircuitBendImageData } from "./engine-core.js";

// One frame per message. The frame buffer arrives transferred (zero-copy)
// and is transferred back; the ghost arrives cloned because the render loop
// reuses ring-buffer frames as ghosts for several consecutive frames.
parentPort.on("message", ({ index, width, height, buffer, preset, liveSeed, ghost }) => {
  try {
    const image = { width, height, data: new Uint8ClampedArray(buffer) };
    const resources = ghost
      ? {
          ghost: {
            width: ghost.width,
            height: ghost.height,
            data: new Uint8ClampedArray(ghost.buffer)
          }
        }
      : {};
    processCircuitBendImageData(image, preset, resources, { liveSeed });
    parentPort.postMessage({ index, buffer: image.data.buffer }, [image.data.buffer]);
  } catch (error) {
    parentPort.postMessage({ index, error: error?.message || String(error) });
  }
});
