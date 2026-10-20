# localgpt-world-audio

The one mapping from the LocalGPT world format's audio to sound: FunDSP
synthesis inside a kira mixer.

It plays what the format carries: an entity's audio, ambience layers and
spatial emitters, and a world's soundtrack with the modulations bound to
it. kira owns the device, the mixing, the clocks and streaming decode;
FunDSP graphs feed it as sources. Every [LocalGPT](https://github.com/localgpt-app/localgpt) app plays a
world through it, so a world sounds the same in each.

License: Apache-2.0.
