//! A CJK fallback font for egui.
//!
//! egui ships Latin, Greek and Cyrillic glyphs and nothing for Chinese,
//! Japanese or Korean, so any CJK text in an egui panel — a prompt typed in
//! Chinese, the agent's reply, a Markdown document in the app's editor —
//! rendered as empty boxes. (Bevy's own text is fine: it discovers system
//! fonts through `system_font_discovery`. egui has its own font system.)
//!
//! This finds a CJK font the operating system already ships and appends it to
//! both egui families at the lowest priority, so it is only consulted for
//! glyphs the default fonts lack — Latin text looks exactly as before. Nothing
//! is bundled: the fonts below are system files, so there is no licence to
//! carry and no download. `$LOCALGPT_CJK_FONT` points at any other font file
//! (a `.ttc` collection uses its first face).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

/// The override for a font somewhere else.
pub const LOCALGPT_CJK_FONT: &str = "LOCALGPT_CJK_FONT";

/// System CJK fonts, in preference order per platform. Simplified-Chinese
/// faces first (Hiragino Sans GB, YaHei, Noto Sans CJK SC as the collection's
/// first face covers it), then broader fallbacks. Every one is part of the
/// OS or its standard CJK font package.
const CANDIDATES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    // Windows
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\msyh.ttf",
    "C:\\Windows\\Fonts\\simsun.ttc",
    // Linux (Debian/Ubuntu, Fedora, Arch, WenQuanYi)
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
];

/// The font to use: the override if it names a file, else the first
/// candidate that exists. Pure over `is_file`, so it tests without fonts.
pub fn choose_font(
    override_path: Option<PathBuf>,
    candidates: &[&str],
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(path) = override_path.filter(|p| is_file(p)) {
        return Some(path);
    }
    candidates.iter().map(PathBuf::from).find(|p| is_file(p))
}

/// Install the fallback once, on the first frame egui is ready. The font is a
/// few tens of megabytes read from local disk, once — a short hitch at
/// launch, never again.
pub fn install_cjk_fallback(mut contexts: EguiContexts, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    *done = true;

    let override_path = std::env::var_os(LOCALGPT_CJK_FONT).map(PathBuf::from);
    let Some(path) = choose_font(override_path, CANDIDATES, Path::is_file) else {
        info!(
            "no CJK font found — Chinese, Japanese and Korean text in panels will show as boxes \
             (set {LOCALGPT_CJK_FONT} to a font file)"
        );
        return;
    };
    match add_cjk_font(ctx, &path) {
        Ok(()) => info!("CJK fallback font: {}", path.display()),
        Err(e) => warn!("couldn't read CJK font {}: {e}", path.display()),
    }
}

/// Append the font at `path` to both egui families at the lowest priority.
/// It takes effect from the next pass — egui applies font changes at the
/// start of one.
pub fn add_cjk_font(ctx: &egui::Context, path: &Path) -> std::io::Result<()> {
    let data = egui::FontData::from_owned(std::fs::read(path)?);
    let families = [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
        .into_iter()
        .map(|family| egui::epaint::text::InsertFontFamily {
            family,
            priority: egui::epaint::text::FontPriority::Lowest,
        })
        .collect();
    ctx.add_font(egui::epaint::text::FontInsert::new(
        "localgpt-cjk",
        data,
        families,
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_wins_when_it_exists() {
        let chosen = choose_font(Some(PathBuf::from("/mine.ttf")), CANDIDATES, |p| {
            p == Path::new("/mine.ttf") || p == Path::new(CANDIDATES[0])
        });
        assert_eq!(chosen, Some(PathBuf::from("/mine.ttf")));
    }

    #[test]
    fn a_missing_override_falls_back_to_the_system_list() {
        let chosen = choose_font(Some(PathBuf::from("/gone.ttf")), CANDIDATES, |p| {
            p == Path::new(CANDIDATES[2])
        });
        assert_eq!(chosen, Some(PathBuf::from(CANDIDATES[2])));
    }

    #[test]
    fn nothing_found_is_none_not_a_panic() {
        assert_eq!(choose_font(None, CANDIDATES, |_| false), None);
    }

    /// The real check, headless: egui cannot draw 你好 out of the box, and can
    /// once the system font is added — in both families, since the editor is
    /// monospace and the prompt panel proportional. Skipped where the OS has
    /// no CJK font (a bare CI runner); on a desktop this is the whole feature.
    #[test]
    fn a_system_cjk_font_makes_chinese_drawable() {
        let Some(path) = choose_font(None, CANDIDATES, Path::is_file) else {
            eprintln!("skipped: no system CJK font here");
            return;
        };
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |_| {});
        let proportional = egui::FontId::proportional(14.0);
        let monospace = egui::FontId::monospace(14.0);
        assert!(
            !ctx.fonts_mut(|f| f.has_glyphs(&proportional, "你好")),
            "egui's defaults unexpectedly cover CJK — this fallback may be redundant"
        );

        add_cjk_font(&ctx, &path).unwrap();
        let _ = ctx.run_ui(Default::default(), |_| {});
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&proportional, "你好世界")));
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&monospace, "你好世界")));
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&proportional, "Latin still fine")));
    }
}
