# Preset Format Draft

Presets should be human-readable JSON and stable enough to share between the web app, future video mode, and a possible command-line tool.

This is a first draft, not a locked schema.

```json
{
  "schemaVersion": 1,
  "appVersion": "0.1.0",
  "name": "Bent CCD-03",
  "cameraModel": "Bent CCD-03",
  "description": "Strong magenta/cyan clipping with medium vertical melt.",
  "tags": ["ccd", "melt", "false-color"],
  "seed": 381941,
  "createdAt": "2026-07-05T00:00:00.000Z",
  "thumbnail": {
    "type": "data-url",
    "mime": "image/webp",
    "data": ""
  },
  "exampleOutputs": [],
  "macros": {
    "bend": 0.78,
    "colorFault": 0.86,
    "melt": 0.58,
    "burn": 0.72,
    "noise": 0.42,
    "cheapness": 0.35,
    "chaos": 0.44
  },
  "pipeline": {
    "cheapCamera": {
      "enabled": true,
      "internalScale": 0.75,
      "bitDepth": 6,
      "sharpen": 0.42
    },
    "exposureFault": {
      "enabled": true,
      "gain": 1.24,
      "blackCrush": 0.28,
      "highlightClip": 0.71,
      "clipColorBias": [1.0, 0.22, 0.88]
    },
    "falseColor": {
      "enabled": true,
      "mode": "solarized-ccd",
      "posterizeLevels": 7,
      "channelSwap": 0.32,
      "hueWarp": 0.64,
      "saturation": 1.85
    },
    "edgeBurn": {
      "enabled": true,
      "strength": 0.52,
      "radius": 1.5,
      "palette": ["cyan", "magenta", "green", "black"]
    },
    "verticalSmear": {
      "enabled": true,
      "strength": 0.63,
      "threshold": 0.58,
      "decay": 0.91,
      "jitter": 0.18,
      "direction": "down"
    },
    "sensorNoise": {
      "enabled": true,
      "amount": 0.31,
      "colorAmount": 0.82,
      "shadowBias": 0.67,
      "speckleSize": 1
    },
    "memoryFault": {
      "enabled": false,
      "blockShift": 0.0,
      "rowRepeat": 0.0,
      "scanlineDropout": 0.0
    },
    "output": {
      "exportScale": 1.0,
      "format": "png",
      "preserveOriginalResolution": true
    }
  }
}
```

## Notes

- Numeric controls should generally use `0.0` to `1.0` ranges unless a physical unit or count is clearer.
- Presets should store macro controls and expanded module controls.
- Macro controls may be recalculated from module values later, but the first version can simply store both.
- Seeds should be included when available, but exact pixel reproduction is not a hard promise.
- Thumbnails can start as embedded data URLs for easy sharing. If preset files become too large, thumbnails can move to separate files later.
