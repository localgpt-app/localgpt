//! LocalGPT Gen — the world builder's binary.
//!
//! Everything lives in [`localgpt_gen::cli`] so `localgpt-app gen …` (which
//! `localgpt gen …` hands off to) shares the exact same parser and dispatch.

fn main() -> anyhow::Result<()> {
    localgpt_gen::cli::main()
}
