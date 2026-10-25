//! Opening the output device does not depend on the caller's stack.
//!
//! Gen's audio thread overflowed its 2 MiB stack opening the device on the
//! Ubuntu 26.04 CI runners, aborting the whole test process; `Engine::new`
//! now opens it on a thread of its own with a main thread's stack. Called
//! here from a far smaller stack than any caller has, it must come back with
//! an engine or a silent world — never abort. Without an output device the
//! test passes trivially; on a machine with one (the runners that found the
//! bug) it goes through the real native audio stack.

use localgpt_world_audio::Engine;

#[test]
fn opening_the_device_does_not_depend_on_the_callers_stack() {
    let engine = std::thread::Builder::new()
        .name("small-stack-caller".into())
        .stack_size(256 * 1024)
        .spawn(Engine::new)
        .expect("spawn the caller")
        .join()
        .expect("the caller came back");
    eprintln!("output device opened: {}", engine.is_some());
    // Dropped here, on the test thread: the engine is `Send`, as callers
    // that hold it in a resource rely on.
    drop(engine);
}
