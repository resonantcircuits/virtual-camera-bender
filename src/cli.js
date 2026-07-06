import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { cpus } from "node:os";
import { dirname, resolve } from "node:path";
import { Worker } from "node:worker_threads";
import { applyMacrosToPipeline, clonePreset, normalizePreset } from "./presets.js";
import { BUILT_IN_PRESETS } from "./built-in-presets.js";
import { processCircuitBendImageData } from "./engine-core.js";
import { prepareTemporalFrame } from "./temporal.js";
import { fitWithin, setAtPath } from "./utils.js";

const args = process.argv.slice(2);

main().catch((error) => {
  fail(error?.stack || String(error));
});

async function main() {
  const command = args[0];
  if (!command || command === "--help" || command === "-h") {
    printHelp();
    return;
  }

  if (command === "list-presets") {
    for (const preset of BUILT_IN_PRESETS) {
      console.log(`${preset.name}\t${preset.description}`);
    }
    return;
  }

  if (command === "render") {
    renderCommand(args.slice(1));
    return;
  }

  if (command === "render-video") {
    await renderVideoCommand(args.slice(1));
    return;
  }

  fail(`Unknown command: ${command}`);
}

function renderCommand(argv) {
  const options = parseArgs(argv);
  if (!options.input || !options.output) {
    printHelp();
    fail("render requires <input> and <output>");
  }
  if (!existsSync(options.input)) {
    fail(`Input image does not exist: ${options.input}`);
  }

  const preset = buildPreset(options);

  const decoded = decodeImage(options.input, options.maxDimension);
  const image = {
    width: decoded.width,
    height: decoded.height,
    data: new Uint8ClampedArray(decoded.data.buffer, decoded.data.byteOffset, decoded.data.byteLength)
  };

  const resources = {};
  if (options.ghostPath) {
    resources.ghost = loadGhostImage(options.ghostPath, options.maxDimension);
  }

  const start = Date.now();
  processCircuitBendImageData(image, preset, resources);
  encodeImage(options.output, image.width, image.height, Buffer.from(image.data));
  const seconds = ((Date.now() - start) / 1000).toFixed(2);
  console.log(
    `Rendered ${options.input} -> ${options.output} (${image.width}x${image.height}, ${preset.name}, ${seconds}s)`
  );
}

// --- Video rendering ---

async function renderVideoCommand(argv) {
  const options = parseArgs(argv);
  if (!options.input || !options.output) {
    printHelp();
    fail("render-video requires <input> and <output>");
  }
  if (!existsSync(options.input)) {
    fail(`Input video does not exist: ${options.input}`);
  }

  const preset = buildPreset(options);
  const meta = probeVideo(options.input);
  const fitted = fitWithin(meta.width, meta.height, options.maxDimension);
  // libx264 with yuv420p needs even dimensions.
  const width = Math.max(2, fitted.width & ~1);
  const height = Math.max(2, fitted.height & ~1);
  const frameSize = width * height * 4;
  const jobs = Math.max(1, options.jobs || Math.max(1, cpus().length - 1));
  const ghostLag = Math.max(0, Math.round(preset.temporal?.ghostFrame || 0));
  const staticGhost = options.ghostPath ? loadGhostImage(options.ghostPath, options.maxDimension) : null;
  const duration = options.duration ?? Math.max(0, meta.duration - (options.start || 0));
  const totalEstimate = Math.max(1, Math.round(duration * meta.fps));

  console.log(
    `Rendering ${options.input} -> ${options.output}\n` +
      `  ${width}x${height} @ ${meta.fps.toFixed(3)} fps, ~${totalEstimate} frames, ` +
      `${jobs} worker${jobs === 1 ? "" : "s"}, preset "${preset.name}", ` +
      `temporal ${preset.temporal.mode}` +
      (preset.temporal.driftAmount > 0 ? `, drift ${preset.temporal.driftAmount}` : "") +
      (ghostLag > 0 ? `, ghost lag ${ghostLag}` : "")
  );

  const decode = spawnDecoder(options, width, height);
  const encode = spawnEncoder(options, width, height, meta);
  const pool = createWorkerPool(jobs);
  // Encoder stdin errors (e.g. bad container/codec combo) surface via the
  // exit promise; without this handler an EPIPE would crash the process.
  encode.child.stdin.on("error", () => {});

  const inputRing = new Map();
  const inFlight = [];
  const pipelineDepth = jobs * 2;
  let frameIndex = 0;
  let written = 0;
  const startedAt = Date.now();

  const writeOldest = async () => {
    const job = inFlight.shift();
    const result = await job.promise;
    if (result.error) fail(`Frame ${result.index} failed: ${result.error}`);
    const buffer = Buffer.from(result.buffer);
    if (!encode.child.stdin.write(buffer)) {
      await Promise.race([once(encode.child.stdin, "drain"), encode.exit]);
    }
    written += 1;
    if (written % 30 === 0) {
      const elapsed = (Date.now() - startedAt) / 1000;
      const fps = written / Math.max(elapsed, 0.001);
      console.log(
        `  frame ${written}/${totalEstimate} (${((written / totalEstimate) * 100).toFixed(0)}%, ${fps.toFixed(1)} fps)`
      );
    }
  };

  try {
    for await (const frame of readFrames(decode.child.stdout, frameSize)) {
      const index = frameIndex++;

      let ghost = staticGhost;
      if (ghostLag > 0) {
        inputRing.set(index, Buffer.from(frame));
        const source = inputRing.get(Math.max(0, index - ghostLag));
        ghost = { width, height, buffer: source.buffer, byteOffset: source.byteOffset, byteLength: source.byteLength };
        for (const key of inputRing.keys()) {
          if (key < index - ghostLag + 1) inputRing.delete(key);
        }
      }

      while (inFlight.length >= pipelineDepth) await writeOldest();

      const frameState = prepareTemporalFrame(preset, index);
      inFlight.push({
        index,
        promise: pool.run(
          {
            index,
            width,
            height,
            buffer: frame.buffer,
            preset: frameState.preset,
            liveSeed: frameState.liveSeed,
            ghost: ghost ? ghostPayload(ghost) : null
          },
          [frame.buffer]
        )
      });
    }

    while (inFlight.length) await writeOldest();
  } finally {
    pool.destroy();
  }

  encode.child.stdin.end();
  const decodeStatus = await decode.exit;
  if (decodeStatus !== 0) {
    fail(`ffmpeg decode failed:\n${decode.stderr()}`);
  }
  const encodeStatus = await encode.exit;
  if (encodeStatus !== 0) {
    fail(`ffmpeg encode failed:\n${encode.stderr()}`);
  }

  const seconds = ((Date.now() - startedAt) / 1000).toFixed(1);
  console.log(`Rendered ${written} frames in ${seconds}s -> ${options.output}`);
}

// Ghost payloads are cloned (not transferred): ring-buffer frames serve as
// the ghost for several consecutive frames, and the static ghost for all.
function ghostPayload(ghost) {
  if (ghost.data) {
    return { width: ghost.width, height: ghost.height, buffer: Buffer.from(ghost.data).buffer };
  }
  return {
    width: ghost.width,
    height: ghost.height,
    buffer: ghost.buffer.slice(ghost.byteOffset, ghost.byteOffset + ghost.byteLength)
  };
}

function spawnDecoder(options, width, height) {
  const decodeArgs = ["-hide_banner", "-loglevel", "error"];
  if (options.start) decodeArgs.push("-ss", String(options.start));
  if (options.duration != null) decodeArgs.push("-t", String(options.duration));
  decodeArgs.push(
    "-i",
    options.input,
    "-vf",
    `scale=${width}:${height}:flags=lanczos`,
    "-an",
    "-f",
    "rawvideo",
    "-pix_fmt",
    "rgba",
    "pipe:1"
  );
  return spawnStreaming("ffmpeg", decodeArgs, "pipe");
}

function spawnEncoder(options, width, height, meta) {
  const isMovContainer = /\.(mp4|mov|m4v)$/i.test(options.output);
  const encodeArgs = [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-f",
    "rawvideo",
    "-pix_fmt",
    "rgba",
    "-s",
    `${width}x${height}`,
    "-r",
    meta.fpsRational,
    "-i",
    "pipe:0"
  ];
  if (options.start) encodeArgs.push("-ss", String(options.start));
  if (options.duration != null) encodeArgs.push("-t", String(options.duration));
  encodeArgs.push("-i", options.input, "-map", "0:v");
  if (meta.hasAudio) encodeArgs.push("-map", "1:a?", "-c:a", "copy");
  encodeArgs.push("-c:v", "libx264", "-crf", String(options.crf ?? 18), "-pix_fmt", "yuv420p", "-shortest");
  if (isMovContainer) encodeArgs.push("-movflags", "+faststart");
  encodeArgs.push(options.output);
  mkdirSync(dirname(options.output), { recursive: true });
  return spawnStreaming("ffmpeg", encodeArgs, "pipe");
}

function spawnStreaming(binary, spawnArgs, stdio) {
  const child = spawn(binary, spawnArgs, { stdio: ["pipe", stdio === "pipe" ? "pipe" : "ignore", "pipe"] });
  const stderrChunks = [];
  child.stderr.on("data", (chunk) => stderrChunks.push(chunk));
  const exit = new Promise((resolvePromise) => {
    child.on("close", (code) => resolvePromise(code ?? 1));
    child.on("error", () => resolvePromise(1));
  });
  return {
    child,
    exit,
    stderr: () => Buffer.concat(stderrChunks).toString()
  };
}

// Reassembles the decoder's byte stream into frame-sized buffers. Each
// buffer is standalone (allocUnsafe above the pool threshold), so it can be
// transferred to a worker without detaching anything shared.
async function* readFrames(stream, frameSize) {
  let frame = Buffer.allocUnsafe(frameSize);
  let offset = 0;
  for await (const chunk of stream) {
    let position = 0;
    while (position < chunk.length) {
      const take = Math.min(chunk.length - position, frameSize - offset);
      chunk.copy(frame, offset, position, position + take);
      offset += take;
      position += take;
      if (offset === frameSize) {
        yield frame;
        frame = Buffer.allocUnsafe(frameSize);
        offset = 0;
      }
    }
  }
  if (offset !== 0) {
    throw new Error(`Decoder stream ended mid-frame (${offset}/${frameSize} bytes)`);
  }
}

function createWorkerPool(size) {
  const idle = [];
  const waiters = [];
  const workers = [];

  for (let i = 0; i < size; i += 1) {
    const worker = new Worker(new URL("./video-worker.js", import.meta.url));
    worker.unref();
    workers.push(worker);
    idle.push(worker);
  }

  const acquire = () => {
    if (idle.length) return Promise.resolve(idle.pop());
    return new Promise((resolvePromise) => waiters.push(resolvePromise));
  };

  const release = (worker) => {
    const waiter = waiters.shift();
    if (waiter) waiter(worker);
    else idle.push(worker);
  };

  return {
    async run(payload, transfers) {
      const worker = await acquire();
      return new Promise((resolvePromise, rejectPromise) => {
        const onMessage = (message) => {
          cleanup();
          release(worker);
          resolvePromise(message);
        };
        const onError = (error) => {
          cleanup();
          rejectPromise(error);
        };
        const cleanup = () => {
          worker.off("message", onMessage);
          worker.off("error", onError);
        };
        worker.on("message", onMessage);
        worker.on("error", onError);
        worker.postMessage(payload, transfers);
      });
    },
    destroy() {
      workers.forEach((worker) => worker.terminate());
    }
  };
}

function probeVideo(input) {
  const result = spawnSync(
    "ffprobe",
    ["-v", "error", "-show_streams", "-show_format", "-of", "json", input],
    { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 }
  );
  if (result.status !== 0) {
    fail(`ffprobe failed:\n${result.stderr}`);
  }

  const parsed = JSON.parse(result.stdout);
  const video = parsed.streams?.find((stream) => stream.codec_type === "video");
  if (!video?.width || !video?.height) {
    fail(`Could not read video dimensions for ${input}`);
  }

  const fpsRational = video.r_frame_rate && video.r_frame_rate !== "0/0" ? video.r_frame_rate : "30/1";
  const [numerator, denominator] = fpsRational.split("/").map(Number);
  const fps = denominator ? numerator / denominator : numerator || 30;
  const duration = Number(video.duration) || Number(parsed.format?.duration) || 0;

  return {
    width: video.width,
    height: video.height,
    fps,
    fpsRational,
    duration,
    hasAudio: parsed.streams?.some((stream) => stream.codec_type === "audio") || false
  };
}

// --- Shared option and preset handling ---

function buildPreset(options) {
  const preset = loadPreset(options);

  for (const [key, value] of options.macros) {
    preset.macros[key] = parseValue(value);
  }
  if (options.macros.length) {
    applyMacrosToPipeline(preset);
  }

  for (const [path, value] of options.sets) {
    setAtPath(preset, path, parseValue(value));
  }
  return preset;
}

function loadGhostImage(ghostPath, maxDimension) {
  if (!existsSync(ghostPath)) {
    fail(`Ghost image does not exist: ${ghostPath}`);
  }
  const ghost = decodeImage(ghostPath, maxDimension);
  return {
    width: ghost.width,
    height: ghost.height,
    data: new Uint8ClampedArray(ghost.data.buffer, ghost.data.byteOffset, ghost.data.byteLength)
  };
}

function parseArgs(argv) {
  const options = {
    input: null,
    output: null,
    presetPath: null,
    builtin: BUILT_IN_PRESETS[0].name,
    maxDimension: null,
    ghostPath: null,
    jobs: null,
    start: null,
    duration: null,
    crf: null,
    sets: [],
    macros: []
  };

  const positional = [];
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--preset") {
      options.presetPath = argv[++index];
    } else if (arg === "--builtin") {
      options.builtin = argv[++index];
    } else if (arg === "--max-dimension") {
      options.maxDimension = Number(argv[++index]);
    } else if (arg === "--ghost") {
      options.ghostPath = resolve(argv[++index]);
    } else if (arg === "--jobs") {
      options.jobs = Number(argv[++index]);
    } else if (arg === "--start") {
      options.start = Number(argv[++index]);
    } else if (arg === "--duration") {
      options.duration = Number(argv[++index]);
    } else if (arg === "--crf") {
      options.crf = Number(argv[++index]);
    } else if (arg === "--set") {
      options.sets.push(splitAssignment(argv[++index], "--set"));
    } else if (arg === "--macro") {
      options.macros.push(splitAssignment(argv[++index], "--macro"));
    } else if (arg.startsWith("--set=")) {
      options.sets.push(splitAssignment(arg.slice("--set=".length), "--set"));
    } else if (arg.startsWith("--macro=")) {
      options.macros.push(splitAssignment(arg.slice("--macro=".length), "--macro"));
    } else if (arg.startsWith("--")) {
      fail(`Unknown option: ${arg}`);
    } else {
      positional.push(arg);
    }
  }

  options.input = positional[0] ? resolve(positional[0]) : null;
  options.output = positional[1] ? resolve(positional[1]) : null;
  if (Number.isNaN(options.maxDimension) || options.maxDimension <= 0) {
    options.maxDimension = null;
  }
  if (!Number.isFinite(options.jobs) || options.jobs <= 0) options.jobs = null;
  if (!Number.isFinite(options.start) || options.start <= 0) options.start = null;
  if (!Number.isFinite(options.duration) || options.duration <= 0) options.duration = null;
  if (!Number.isFinite(options.crf) || options.crf < 0) options.crf = null;
  return options;
}

function splitAssignment(value, optionName) {
  const splitAt = value.indexOf("=");
  if (splitAt === -1) {
    fail(`${optionName} expects path=value`);
  }
  return [value.slice(0, splitAt), value.slice(splitAt + 1)];
}

function parseValue(value) {
  if (value === "true") return true;
  if (value === "false") return false;
  if (value === "null") return null;
  if (value !== "" && !Number.isNaN(Number(value))) return Number(value);
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function loadPreset(options) {
  if (options.presetPath) {
    return normalizePreset(JSON.parse(readFileSync(resolve(options.presetPath), "utf8")));
  }

  const name = options.builtin.toLowerCase();
  const preset = BUILT_IN_PRESETS.find((item) => item.name.toLowerCase() === name);
  if (!preset) {
    fail(`Unknown built-in preset: ${options.builtin}`);
  }
  return clonePreset(preset);
}

function decodeImage(input, maxDimension) {
  const metadata = probeImage(input);
  const fitted = fitWithin(metadata.width, metadata.height, maxDimension);
  const args = [
    "-hide_banner",
    "-loglevel",
    "error",
    "-noautorotate",
    "-i",
    input,
    "-frames:v",
    "1"
  ];

  if (fitted.width !== metadata.width || fitted.height !== metadata.height) {
    args.push("-vf", `scale=${fitted.width}:${fitted.height}:flags=lanczos`);
  }

  args.push("-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1");

  const result = spawnSync("ffmpeg", args, {
    maxBuffer: Math.max(fitted.width * fitted.height * 4 + 1024 * 1024, 512 * 1024 * 1024)
  });

  if (result.status !== 0) {
    fail(`ffmpeg decode failed:\n${result.stderr.toString()}`);
  }

  const expected = fitted.width * fitted.height * 4;
  if (result.stdout.length !== expected) {
    fail(`Decoded byte count mismatch: expected ${expected}, got ${result.stdout.length}`);
  }

  return {
    width: fitted.width,
    height: fitted.height,
    data: result.stdout
  };
}

function encodeImage(output, width, height, data) {
  mkdirSync(dirname(output), { recursive: true });
  const result = spawnSync(
    "ffmpeg",
    [
      "-hide_banner",
      "-loglevel",
      "error",
      "-y",
      "-f",
      "rawvideo",
      "-pix_fmt",
      "rgba",
      "-s",
      `${width}x${height}`,
      "-i",
      "pipe:0",
      "-frames:v",
      "1",
      output
    ],
    {
      input: data,
      maxBuffer: 16 * 1024 * 1024
    }
  );

  if (result.status !== 0) {
    fail(`ffmpeg encode failed:\n${result.stderr.toString()}`);
  }
}

function probeImage(input) {
  const result = spawnSync(
    "ffprobe",
    [
      "-v",
      "error",
      "-select_streams",
      "v:0",
      "-show_entries",
      "stream=width,height",
      "-of",
      "json",
      input
    ],
    { encoding: "utf8" }
  );

  if (result.status !== 0) {
    fail(`ffprobe failed:\n${result.stderr}`);
  }

  const stream = JSON.parse(result.stdout).streams?.[0];
  if (!stream?.width || !stream?.height) {
    fail(`Could not read image dimensions for ${input}`);
  }

  return {
    width: stream.width,
    height: stream.height
  };
}

function printHelp() {
  console.log(`Virtual Camera Bender CLI

Usage:
  node src/cli.js list-presets
  node src/cli.js render <input-image> <output-image> [options]
  node src/cli.js render-video <input-video> <output-video> [options]

Options:
  --builtin <name>            Built-in preset name. Default: Bent CCD-03
  --preset <file.json>        Preset JSON file to load
  --max-dimension <pixels>    Resize input before processing for test renders
  --ghost <image>             Second image used by the bufferGhost module
  --macro key=value           Override a macro before rendering
  --set path=value            Override any preset field after macro mapping
                              (includes temporal.*, e.g. --set temporal.mode=hold)

Video options (render-video only):
  --jobs <n>                  Parallel render workers. Default: CPU cores - 1
  --start <seconds>           Trim: start offset into the input
  --duration <seconds>        Trim: how many seconds to render
  --crf <n>                   x264 quality (lower = better). Default: 18

Video temporal behavior comes from the preset's "temporal" block:
  mode (locked|hold|flicker), holdFrames, driftAmount, driftSpeed, ghostFrame.
  ghostFrame > 0 feeds the bufferGhost module the input frame N back, and
  overrides --ghost. Output video: same fps as source, H.264 + yuv420p;
  audio is stream-copied. Use .mp4/.mov/.mkv outputs.

Examples:
  node src/cli.js render test-images/XT307819.jpeg out.png --builtin "Overheated Sensor" --max-dimension 1600
  node src/cli.js render input.jpg out.png --set pipeline.verticalSmear.length=1 --set pipeline.verticalSmear.decay=0.997
  node src/cli.js render-video clip.mp4 bent.mp4 --preset my-camera.vcb-preset.json --set temporal.mode=hold --set temporal.driftAmount=0.4
  node src/cli.js render-video clip.mp4 test.mp4 --builtin "Codec Rot" --start 2 --duration 3 --max-dimension 960
`);
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
