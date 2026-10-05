---
title: "Behaviors"
---

# Behaviors

Behaviors are data-driven animations that stack on entities — no scripting required. Multiple behaviors can be combined on a single entity for complex motion.

## Behavior Types

### `orbit`

Rotate around a center point or entity.

```json
{
  "type": "orbit",
  "center": "sun",           // Entity name to orbit around (optional)
  "center_point": [0, 0, 0], // Or explicit center point
  "radius": 5.0,
  "speed": 30.0,             // Degrees per second
  "axis": [0, 1, 0],         // Rotation axis (default: Y)
  "phase": 0.0,              // Starting angle in degrees
  "tilt": 0.0                // Tilt the orbit plane in degrees
}
```

### `spin`

Continuous rotation around local axes.

```json
{
  "type": "spin",
  "axis": [0, 1, 0],
  "speed": 90.0              // Degrees per second
}
```

### `bob`

Sinusoidal oscillation along an axis.

```json
{
  "type": "bob",
  "axis": [0, 1, 0],
  "amplitude": 1.0,
  "frequency": 1.0,          // Hz
  "phase": 0.0               // Starting phase in degrees
}
```

### `look_at`

Continuously face toward a target entity.

```json
{
  "type": "look_at",
  "target": "camera"         // Entity name to track
}
```

### `pulse`

Scale breathing effect.

```json
{
  "type": "pulse",
  "min_scale": 0.9,
  "max_scale": 1.1,
  "frequency": 2.0           // Hz
}
```

### `path_follow`

Follow a path of waypoints in sequence.

```json
{
  "type": "path_follow",
  "waypoints": [[0, 0, 0], [5, 0, 0], [5, 0, 5], [0, 0, 5]],
  "speed": 2.0,              // Units per second
  "mode": "loop",            // "loop", "ping_pong", or "once"
  "orient_to_path": true     // Face direction of movement
}
```

### `bounce`

Gravity-based bouncing with damping.

```json
{
  "type": "bounce",
  "height": 3.0,             // Initial bounce height
  "gravity": 9.8,            // Acceleration (units/s²)
  "damping": 0.7,            // Energy retained per bounce (0-1)
  "surface_y": 0.0           // Ground level
}
```

## Composing Behaviors

Multiple behaviors can be added to a single entity. They execute in sequence each frame:

```json
// Add orbit around a point
gen_add_behavior({
  "entity": "planet",
  "behavior": {
    "type": "orbit",
    "center_point": [0, 0, 0],
    "radius": 10.0,
    "speed": 20.0
  }
})

// Add spin on its own axis
gen_add_behavior({
  "entity": "planet",
  "behavior": {
    "type": "spin",
    "axis": [0, 1, 0],
    "speed": 45.0
  }
})

// Add subtle bob for atmosphere
gen_add_behavior({
  "entity": "planet",
  "behavior": {
    "type": "bob",
    "axis": [0, 1, 0],
    "amplitude": 0.2,
    "frequency": 0.5
  }
})
```

## Managing Behaviors

```json
// List all behaviors on an entity
gen_list_behaviors({ "entity": "planet" })

// Remove a specific behavior
gen_remove_behavior({
  "entity": "planet",
  "behavior_id": "orbit_0"
})

// Pause all behaviors globally
gen_pause_behaviors({ "pause": true })

// Resume behaviors
gen_pause_behaviors({ "pause": false })
```
