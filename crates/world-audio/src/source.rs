//! A FunDSP graph as a kira sound, which is the join between the two halves.
//!
//! Gen ran its graphs on its own cpal stream with a management thread and
//! lock-free `Shared<f32>` parameters. Everything below the graph goes away
//! here: kira owns the device, the mixer, the tweens and — for an emitter with
//! a radius — the spatial track and listener that Gen attenuated by hand. The
//! `Shared` parameters still work, because they are just atomics the graph
//! reads; a caller keeps its handles and writes to them from anywhere.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fundsp::prelude::AudioUnit;
use kira::Frame;
use kira::info::Info;
use kira::sound::{Sound, SoundData};

/// Plays a mono FunDSP graph, centred. Infinite: it finishes when its handle is
/// dropped, which is how kira models a voice that runs until stopped.
pub struct FundspSound {
    graph: Box<dyn AudioUnit>,
    /// Set when the handle drops. Shared rather than a channel so `finished`
    /// stays allocation-free on the audio thread.
    stopped: Arc<AtomicBool>,
    /// The rate the graph was told to run at, so a device that reports a
    /// different one is caught rather than quietly detuning every voice.
    sample_rate: f64,
}

/// Dropping this stops the sound.
pub struct FundspHandle {
    stopped: Arc<AtomicBool>,
}

impl Drop for FundspHandle {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// What you hand to `AudioManager::play`.
pub struct Fundsp {
    graph: Box<dyn AudioUnit>,
    sample_rate: f64,
}

impl Fundsp {
    /// `graph` must be mono (one output channel) and is reset to `sample_rate`.
    pub fn new(mut graph: Box<dyn AudioUnit>, sample_rate: f64) -> Self {
        graph.set_sample_rate(sample_rate);
        Self { graph, sample_rate }
    }
}

impl SoundData for Fundsp {
    type Error = std::convert::Infallible;
    type Handle = FundspHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let stopped = Arc::new(AtomicBool::new(false));
        let handle = FundspHandle {
            stopped: Arc::clone(&stopped),
        };
        Ok((
            Box::new(FundspSound {
                graph: self.graph,
                stopped,
                sample_rate: self.sample_rate,
            }),
            handle,
        ))
    }
}

impl Sound for FundspSound {
    fn process(&mut self, out: &mut [Frame], dt: f64, _info: &Info) {
        // kira states dt per frame; if the device disagrees with what the graph
        // was built for, retune rather than play at the wrong pitch.
        let rate = 1.0 / dt;
        if (rate - self.sample_rate).abs() > 1.0 {
            self.graph.set_sample_rate(rate);
            self.sample_rate = rate;
        }
        if self.stopped.load(Ordering::Relaxed) {
            out.fill(Frame::ZERO);
            return;
        }
        // Mono graph, centred. A NaN would propagate through the mixer and
        // silence everything, so it is clamped here where it is cheap.
        for frame in out.iter_mut() {
            let sample = self.graph.get_mono();
            let sample = if sample.is_finite() { sample } else { 0.0 };
            *frame = Frame::new(sample, sample);
        }
    }

    fn finished(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use localgpt_world_types as wt;

    const RATE: f64 = 48_000.0;

    /// kira has no `Info::mock()`; a test one is built through
    /// `MockInfoBuilder`.
    fn mock_info() -> Info<'static> {
        kira::info::MockInfoBuilder::new().build()
    }

    fn sound_for(source: &wt::AudioSource) -> (Box<dyn Sound>, FundspHandle) {
        let graph = crate::graphs::build(source).expect("synthesisable");
        Fundsp::new(graph, RATE).into_sound().unwrap()
    }

    #[test]
    fn every_synthesised_source_produces_finite_audio() {
        // The graphs are the part a bad port would break silently: a NaN or an
        // all-zero voice both "work" until you listen.
        for source in [
            wt::AudioSource::Wind {
                speed: 0.5,
                gustiness: 0.5,
            },
            wt::AudioSource::Rain { intensity: 0.7 },
            wt::AudioSource::Forest {
                bird_density: 0.5,
                wind: 0.3,
            },
            wt::AudioSource::Ocean { wave_size: 0.6 },
            wt::AudioSource::Cave {
                drip_rate: 0.4,
                resonance: 0.5,
            },
            wt::AudioSource::Stream { flow_rate: 0.5 },
            wt::AudioSource::Water { turbulence: 0.6 },
            wt::AudioSource::Fire {
                intensity: 0.6,
                crackle: 0.7,
            },
            wt::AudioSource::Hum {
                frequency: 60.0,
                warmth: 0.5,
            },
            wt::AudioSource::WindEmitter { pitch: 1.0 },
            wt::AudioSource::Custom {
                waveform: wt::WaveformType::PinkNoise,
                filter_cutoff: 800.0,
                filter_type: wt::FilterType::Lowpass,
            },
        ] {
            let (mut sound, _handle) = sound_for(&source);
            let mut out = [Frame::ZERO; 512];
            let info = mock_info();
            sound.process(&mut out, 1.0 / RATE, &info);
            assert!(
                out.iter()
                    .all(|f| f.left.is_finite() && f.right.is_finite()),
                "{source:?} produced a non-finite sample",
            );
            assert!(
                out.iter().any(|f| f.left != 0.0),
                "{source:?} produced pure silence",
            );
        }
    }

    #[test]
    fn dropping_the_handle_finishes_the_sound() {
        let (mut sound, handle) = sound_for(&wt::AudioSource::Rain { intensity: 0.5 });
        assert!(!sound.finished());
        drop(handle);
        assert!(
            sound.finished(),
            "a dropped handle must let kira unload the voice"
        );
        let mut out = [Frame::ZERO; 8];
        sound.process(&mut out, 1.0 / RATE, &mock_info());
        assert!(
            out.iter().all(|f| f.left == 0.0),
            "a stopped voice must go quiet"
        );
    }

    #[test]
    fn non_synthesised_sources_have_no_graph() {
        // kira decodes File itself; Abc is unimplemented; Silence is no voice.
        for source in [
            wt::AudioSource::File {
                path: "x.mp3".into(),
                looping: true,
            },
            wt::AudioSource::Abc {
                notation: "CDEF".into(),
            },
            wt::AudioSource::Silence,
        ] {
            assert!(crate::graphs::build(&source).is_none(), "{source:?}");
        }
    }

    #[test]
    fn names_infer_the_emitter_gen_inferred() {
        use wt::AudioSource as S;
        assert!(matches!(
            crate::graphs::infer_emitter_from_name("campfire").map(|(s, _)| s),
            Some(S::Fire { .. })
        ));
        assert!(matches!(
            crate::graphs::infer_emitter_from_name("great waterfall").map(|(s, _)| s),
            Some(S::Water { .. })
        ));
        assert!(matches!(
            crate::graphs::infer_emitter_from_name("old generator").map(|(s, _)| s),
            Some(S::Hum { .. })
        ));
        assert!(crate::graphs::infer_emitter_from_name("a plain cube").is_none());
    }
}
