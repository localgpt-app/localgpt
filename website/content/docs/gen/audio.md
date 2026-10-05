---
title: "Audio"
---

# Audio

LocalGPT Gen includes a procedural environmental audio system built on FunDSP. The system synthesizes natural-sounding ambient soundscapes and spatial sound emitters that respond to camera position in real-time.

## Ambient Sounds

Global soundscapes that loop continuously with natural variation:

| Sound | Description |
|-------|-------------|
| `wind` | Pink noise with LFO-modulated lowpass (speed, gustiness) |
| `rain` | White noise bandpass with AM modulation (intensity) |
| `forest` | Pink noise layer + sine chirps for birds (bird_density, wind) |
| `ocean` | Brown noise with slow amplitude LFO + foam hiss (wave_size) |
| `cave` | Sine chirps (drips) + quiet brown noise (drip_rate, resonance) |
| `stream` | Layered white/brown noise + bandpass (flow_rate) |
| `silence` | No ambient sound |

All ambient sounds use LFO modulation (0.05–0.3 Hz) to ensure natural variation — no two moments sound identical.

### Setting Ambience

```json
gen_set_ambience({
  "layers": [
    {
      "name": "wind",
      "sound": { "type": "wind", "speed": 0.5, "gustiness": 0.3 },
      "volume": 0.6
    },
    {
      "name": "birds",
      "sound": { "type": "forest", "bird_density": 0.4, "wind": 0.2 },
      "volume": 0.4
    }
  ],
  "master_volume": 0.8
})
```

## Emitter Sounds

Spatial audio sources that respond to camera distance and direction:

| Sound | Description |
|-------|-------------|
| `water` | White noise bandpass + brown undertone (turbulence) |
| `fire` | Brown rumble + noise bursts (intensity, crackle) |
| `hum` | Sine + harmonics with detune (frequency, warmth) |
| `wind` | Pink noise with LFO modulation (pitch) |
| `custom` | Direct waveform → filter (waveform, filter_cutoff, filter_type) |

Emitters support spatial rendering:
- **Volume attenuation:** Quadratic falloff within radius
- **Stereo panning:** Left/right based on camera relative direction

### Creating Emitters

```json
gen_audio_emitter({
  "name": "campfire_sound",
  "entity": "campfire",      // Attach to existing entity
  "sound": { "type": "fire", "intensity": 0.6, "crackle": 0.5 },
  "radius": 15.0,
  "volume": 0.8
})
```

Or position standalone:

```json
gen_audio_emitter({
  "name": "waterfall",
  "position": [10.0, 0.0, 5.0],
  "sound": { "type": "water", "turbulence": 0.8 },
  "radius": 20.0,
  "volume": 0.7
})
```

### Modifying Emitters

```json
gen_modify_audio({
  "name": "campfire_sound",
  "volume": 0.9,
  "sound": { "type": "fire", "intensity": 0.8, "crackle": 0.6 }
})
```

## Auto-Inference

The system automatically detects entity names and assigns audio:

| Keywords | Sound |
|----------|-------|
| waterfall, fountain | Water (turbulence: 0.8) |
| river, water | Water (turbulence: 0.5) |
| stream, creek, brook | Water (turbulence: 0.3) |
| fire, campfire, torch, flame, bonfire | Fire |
| generator, machine, engine, motor | Hum |
| vent, fan, wind_turbine | Wind |

To override auto-inference, call `gen_audio_emitter` explicitly with a different sound.

## Querying Audio State

```json
gen_audio_info({})
```

Returns current ambience layers, active emitters, volumes, and positions.
