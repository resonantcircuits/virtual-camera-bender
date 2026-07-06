import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import {
  applyMacrosToPipeline,
  BUILT_IN_PRESETS,
  clonePreset,
  normalizePreset
} from "./presets.js";
import { processCircuitBendImageData } from "./engine-core.js";
import { fitWithin, setAtPath } from "./utils.js";

const args = process.argv.slice(2);

main();

function main() {
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

  if (command !== "render") {
    fail(`Unknown command: ${command}`);
  }

  renderCommand(args.slice(1));
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

  const decoded = decodeImage(options.input, options.maxDimension);
  const image = {
    width: decoded.width,
    height: decoded.height,
    data: new Uint8ClampedArray(decoded.data.buffer, decoded.data.byteOffset, decoded.data.byteLength)
  };

  const resources = {};
  if (options.ghostPath) {
    if (!existsSync(options.ghostPath)) {
      fail(`Ghost image does not exist: ${options.ghostPath}`);
    }
    const ghost = decodeImage(options.ghostPath, options.maxDimension);
    resources.ghost = {
      width: ghost.width,
      height: ghost.height,
      data: new Uint8ClampedArray(ghost.data.buffer, ghost.data.byteOffset, ghost.data.byteLength)
    };
  }

  const start = Date.now();
  processCircuitBendImageData(image, preset, resources);
  encodeImage(options.output, image.width, image.height, Buffer.from(image.data));
  const seconds = ((Date.now() - start) / 1000).toFixed(2);
  console.log(
    `Rendered ${options.input} -> ${options.output} (${image.width}x${image.height}, ${preset.name}, ${seconds}s)`
  );
}

function parseArgs(argv) {
  const options = {
    input: null,
    output: null,
    presetPath: null,
    builtin: BUILT_IN_PRESETS[0].name,
    maxDimension: null,
    ghostPath: null,
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

Options:
  --builtin <name>            Built-in preset name. Default: Bent CCD-03
  --preset <file.json>        Preset JSON file to load
  --max-dimension <pixels>    Resize input before processing for test renders
  --ghost <image>             Second image used by the bufferGhost module
  --macro key=value           Override a macro before rendering
  --set path=value            Override any preset field after macro mapping

Examples:
  node src/cli.js render test-images/XT307819.jpeg out.png --builtin "Overheated Sensor" --max-dimension 1600
  node src/cli.js render input.jpg out.png --set pipeline.verticalSmear.length=1 --set pipeline.verticalSmear.decay=0.997
`);
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
