#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyphs {
    pub done: char,
    pub failed: char,
    pub bead: char,
    pub waiting_bead: char,
    pub still_going: char,
    pub running_mark: char,
    pub root_switch_mark: char,
    pub ellipsis: &'static str,
    pub chain_link: &'static str,
}

const DONE: [char; 2] = ['√', '+'];
const FAILED: [char; 2] = ['×', 'x'];
const BEAD: [char; 2] = ['■', '#'];
const WAITING_BEAD: [char; 2] = ['·', '.'];
const STILL_GOING: [char; 2] = ['▶', '>'];
const RUNNING_MARK: [char; 3] = ['►', '▶', '>'];
const ROOT_SWITCH_MARK: [char; 2] = ['│', '|'];
const ELLIPSIS: [&str; 2] = ["…", "..."];
const CHAIN_LINK: [&str; 2] = ["→", "->"];

impl Default for Glyphs {
    fn default() -> Self {
        Self::fitting(|_| true)
    }
}

impl Glyphs {
    pub fn fitting(covers: impl Fn(char) -> bool) -> Self {
        let symbol = |candidates: &[char]| {
            candidates
                .iter()
                .copied()
                .find(|candidate| covers(*candidate))
                .unwrap_or(candidates[candidates.len() - 1])
        };
        let text = |candidates: &[&'static str]| {
            candidates
                .iter()
                .copied()
                .find(|candidate| candidate.chars().all(&covers))
                .unwrap_or(candidates[candidates.len() - 1])
        };
        Self {
            done: symbol(&DONE),
            failed: symbol(&FAILED),
            bead: symbol(&BEAD),
            waiting_bead: symbol(&WAITING_BEAD),
            still_going: symbol(&STILL_GOING),
            running_mark: symbol(&RUNNING_MARK),
            root_switch_mark: symbol(&ROOT_SWITCH_MARK),
            ellipsis: text(&ELLIPSIS),
            chain_link: text(&CHAIN_LINK),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAT2_TERMINUS16: &str = "×■●▶·•…│├└─━█▒░↑↓←→≥";
    const DEFAULT8X16: &str = "√■○►▶·•│├└─▌▐█▓▒░▀▄↑↓←→≥";

    fn covered_by(font: &'static str) -> impl Fn(char) -> bool {
        move |symbol| symbol.is_ascii() || font.contains(symbol)
    }

    #[test]
    fn a_font_without_the_square_root_and_pointer_falls_back() {
        let glyphs = Glyphs::fitting(covered_by(LAT2_TERMINUS16));
        assert_eq!(glyphs.done, '+');
        assert_eq!(glyphs.running_mark, '▶');
        assert_eq!(glyphs.failed, '×');
        assert_eq!(glyphs.ellipsis, "…");
    }

    #[test]
    fn the_built_in_kernel_font_loses_the_cross_and_ellipsis() {
        let glyphs = Glyphs::fitting(covered_by(DEFAULT8X16));
        assert_eq!(glyphs.done, '√');
        assert_eq!(glyphs.failed, 'x');
        assert_eq!(glyphs.ellipsis, "...");
        assert_eq!(glyphs.chain_link, "→");
    }

    #[test]
    fn full_unicode_keeps_every_preferred_glyph() {
        let glyphs = Glyphs::default();
        assert_eq!((glyphs.done, glyphs.running_mark), ('√', '►'));
    }
}
