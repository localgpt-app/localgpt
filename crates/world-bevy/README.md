# localgpt-world-bevy

The one [Bevy](https://bevyengine.org) mapping of the LocalGPT world
format: meshes, materials, lights, environment and camera.

Gen, MD and Verse build their scenes through it, so a manifest looks the
same in every [LocalGPT](https://github.com/localgpt-app/localgpt) app, and the web viewer in
`localgpt-world-export` follows the same rules: colours are sRGB except
`emissive`, which is linear; rotations are XYZ Euler angles in degrees;
directional light is in lux, point and spot lights in lumens; every
parametric shape is centered on its origin.

License: Apache-2.0.
