//! Smart search, without being presumptuous:
//! - unambiguous words apply at once: resolutions (4k), ultrawide/16:9,
//!   purity (sfw, sketchy, nsfw);
//! - ambiguous words (colors, categories, sort words, portrait) are only
//!   *suggested*, since "black rock shooter" isn't about the color black,
//!   unless the whole query is filter words ("dark blue 4k");
//! - explicit `color:`, `ratio:`, `res:`, `sort:`, `cat:`, `purity:` always apply;
//! - "quoted phrases" stay literal.

use crate::wallhaven::Sorting;

/// A search that stands alone on Wallhaven (it can't be combined with other
/// words): a tag, wallpapers like one, or a user's uploads. Shown as a named
/// pill instead of a raw `id:123` token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Scope {
    Tag(u64),
    Like(String),
    User(String),
}

impl Scope {
    /// A whole query that is exactly one scope token.
    pub(super) fn parse(query: &str) -> Option<Scope> {
        let q = query.trim();
        if q.contains(char::is_whitespace) {
            return None;
        }
        let lower = q.to_lowercase();
        if let Some(id) = lower.strip_prefix("id:") {
            return id.parse().ok().map(Scope::Tag);
        }
        if let Some(id) = q.strip_prefix("like:").filter(|v| !v.is_empty()) {
            return Some(Scope::Like(id.to_owned()));
        }
        q.strip_prefix('@').filter(|v| !v.is_empty()).map(|u| Scope::User(u.to_owned()))
    }

    /// What Wallhaven gets.
    pub(super) fn token(&self) -> String {
        match self {
            Scope::Tag(id) => format!("id:{id}"),
            Scope::Like(id) => format!("like:{id}"),
            Scope::User(name) => format!("@{name}"),
        }
    }
}

/// Filters recognized in the query, layered over the sidebar's for one search.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Overlay {
    pub color: Option<String>,
    pub ratios: Option<String>,
    pub atleast: Option<String>,
    pub purity: Option<[bool; 3]>,
    pub categories: Option<[bool; 3]>,
    pub sorting: Option<Sorting>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChipKind {
    Color,
    Ratio,
    Resolution,
    Purity,
    Category,
    Sort,
    /// Still sent to Wallhaven, just shown nicely: @user, -tag, +tag, like:, id:
    Operator,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Chip {
    pub kind: ChipKind,
    pub label: String,
    /// Hex for color chips, to draw a swatch.
    pub swatch: Option<String>,
    /// The words of the query this chip came from (removing the chip removes these).
    pub words: Vec<String>,
    /// The explicit form, e.g. `color:black`, used when accepting a suggestion.
    pub explicit: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Parsed {
    /// What goes to Wallhaven's `q`.
    pub text: String,
    pub overlay: Overlay,
    /// Applied filters.
    pub chips: Vec<Chip>,
    /// Ambiguous words that could be filters; offered, not applied.
    pub suggestions: Vec<Chip>,
}

/// Color words to Wallhaven's searchable palette.
const COLORS: &[(&str, &str)] = &[
    ("dark red", "990000"),
    ("maroon", "660000"),
    ("crimson", "cc3333"),
    ("red", "cc0000"),
    ("hot pink", "ea4c88"),
    ("pink", "ea4c88"),
    ("magenta", "993399"),
    ("violet", "993399"),
    ("purple", "663399"),
    ("indigo", "333399"),
    ("navy", "333399"),
    ("dark blue", "333399"),
    ("light blue", "0099cc"),
    ("sky blue", "0099cc"),
    ("blue", "0066cc"),
    ("cyan", "66cccc"),
    ("teal", "66cccc"),
    ("turquoise", "66cccc"),
    ("light green", "77cc33"),
    ("dark green", "336600"),
    ("green", "669900"),
    ("yellow", "ffff00"),
    ("golden", "ffcc33"),
    ("orange", "ff6600"),
    ("brown", "663300"),
    ("black", "000000"),
    ("grey", "999999"),
    ("gray", "999999"),
    ("silver", "cccccc"),
    ("white", "ffffff"),
    ("charcoal", "424153"),
];

fn ratio(word: &str) -> Option<(&'static str, &'static str)> {
    Some(match word {
        "ultrawide" | "superwide" | "21:9" | "32:9" => ("21x9,32x9,48x9", "Ultrawide"),
        "widescreen" | "16:9" | "16x9" => ("16x9,16x10", "16:9"),
        "portrait" | "vertical" | "phone" | "mobile" => ("9x16,10x16,9x18", "Portrait"),
        "square" => ("1x1", "Square"),
        _ => return None,
    })
}

fn resolution(word: &str) -> Option<(&'static str, &'static str)> {
    Some(match word {
        "8k" => ("7680x4320", "8K+"),
        "5k" => ("5120x2880", "5K+"),
        "4k" | "uhd" | "2160p" => ("3840x2160", "4K+"),
        "1440p" | "qhd" | "2k" => ("2560x1440", "1440p+"),
        "1080p" | "fhd" => ("1920x1080", "1080p+"),
        _ => return None,
    })
}

fn sorting(word: &str) -> Option<(Sorting, &'static str)> {
    Some(match word {
        "latest" | "newest" | "new" => (Sorting::DateAdded, "Latest"),
        "top" | "best" | "popular" => (Sorting::Toplist, "Top"),
        "random" | "shuffle" => (Sorting::Random, "Random"),
        "hot" | "trending" => (Sorting::Hot, "Hot"),
        "favorites" | "faves" => (Sorting::Favorites, "Favorites"),
        _ => return None,
    })
}

/// Split into words, keeping "quoted phrases" whole (quotes included).
fn tokenize(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in query.chars() {
        match ch {
            '"' => {
                current.push(ch);
                if quoted {
                    tokens.push(std::mem::take(&mut current));
                }
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[derive(Debug, Clone, Copy)]
enum Hit {
    Color(&'static str, &'static str),
    Ratio(&'static str, &'static str),
    Resolution(&'static str, &'static str),
    Purity(usize),
    Category(usize),
    Sort(Sorting, &'static str),
}

const PURITY: [&str; 3] = ["sfw", "sketchy", "nsfw"];
const PURITY_LABEL: [&str; 3] = ["SFW", "Sketchy", "NSFW"];
const CATEGORY: [&str; 3] = ["general", "anime", "people"];
const CATEGORY_LABEL: [&str; 3] = ["General", "Anime", "People"];

/// A bare word, and whether it's ambiguous (could just be a search term).
fn classify(lower: &str) -> Option<(Hit, bool)> {
    if let Some((name, hex)) = COLORS.iter().find(|(n, _)| *n == lower) {
        return Some((Hit::Color(name, hex), true));
    }
    if let Some((value, label)) = ratio(lower) {
        let ambiguous = matches!(label, "Portrait" | "Square");
        return Some((Hit::Ratio(value, label), ambiguous));
    }
    if let Some((value, label)) = resolution(lower) {
        return Some((Hit::Resolution(value, label), false));
    }
    if let Some(ix) = PURITY.iter().position(|p| *p == lower) {
        return Some((Hit::Purity(ix), false));
    }
    if let Some(ix) = CATEGORY.iter().position(|c| *c == lower) {
        return Some((Hit::Category(ix), true));
    }
    sorting(lower).map(|(sort, label)| (Hit::Sort(sort, label), true))
}

/// `key:value` forms, which always apply.
fn classify_explicit(lower: &str) -> Option<Hit> {
    let (key, value) = lower.split_once(':')?;
    match key {
        "color" | "colour" => {
            let name = value.replace('-', " ");
            if let Some((n, h)) = COLORS.iter().find(|(n, _)| *n == name) {
                return Some(Hit::Color(n, h));
            }
            // Any of Wallhaven's palette by hex.
            let hex = value.trim_start_matches('#');
            COLORS.iter().find(|(_, h)| *h == hex).map(|(n, h)| Hit::Color(n, h))
        }
        "ratio" | "aspect" => ratio(value).map(|(v, l)| Hit::Ratio(v, l)),
        "res" | "min" | "resolution" => resolution(value).map(|(v, l)| Hit::Resolution(v, l)),
        "sort" => sorting(value).map(|(s, l)| Hit::Sort(s, l)),
        "cat" | "category" => CATEGORY.iter().position(|c| *c == value).map(Hit::Category),
        "purity" => PURITY.iter().position(|p| *p == value).map(Hit::Purity),
        _ => None,
    }
}

fn explicit_form(hit: Hit, word: &str) -> String {
    match hit {
        Hit::Color(name, _) => format!("color:{}", name.replace(' ', "-")),
        Hit::Ratio(..) => format!("ratio:{word}"),
        Hit::Resolution(..) => format!("res:{word}"),
        Hit::Purity(ix) => format!("purity:{}", PURITY[ix]),
        Hit::Category(ix) => format!("cat:{}", CATEGORY[ix]),
        Hit::Sort(..) => format!("sort:{word}"),
    }
}

fn chip_for(hit: Hit, words: Vec<String>, explicit: String) -> Chip {
    let (kind, label, swatch) = match hit {
        Hit::Color(name, hex) => (ChipKind::Color, name.to_owned(), Some(hex.to_owned())),
        Hit::Ratio(_, label) => (ChipKind::Ratio, label.to_owned(), None),
        Hit::Resolution(_, label) => (ChipKind::Resolution, label.to_owned(), None),
        Hit::Purity(ix) => (ChipKind::Purity, PURITY_LABEL[ix].to_owned(), None),
        Hit::Category(ix) => (ChipKind::Category, CATEGORY_LABEL[ix].to_owned(), None),
        Hit::Sort(_, label) => (ChipKind::Sort, label.to_owned(), None),
    };
    Chip { kind, label, swatch, words, explicit }
}

enum Piece {
    Text(String),
    Filter { hit: Hit, words: Vec<String>, explicit: Option<String>, ambiguous: bool },
}

pub(super) fn parse(query: &str) -> Parsed {
    let tokens = tokenize(query);
    let mut pieces: Vec<Piece> = Vec::new();
    let mut operators: Vec<Chip> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        let lower = tok.to_lowercase();
        if tok.starts_with('"') {
            pieces.push(Piece::Text(tok.trim_matches('"').to_owned()));
            i += 1;
            continue;
        }
        if let Some(hit) = classify_explicit(&lower) {
            pieces.push(Piece::Filter { hit, words: vec![tok.clone()], explicit: Some(tok.clone()), ambiguous: false });
            i += 1;
            continue;
        }
        // Two-word colors first ("dark blue").
        if let Some(next) = tokens.get(i + 1) {
            let pair = format!("{lower} {}", next.to_lowercase());
            if let Some((name, hex)) = COLORS.iter().find(|(n, _)| *n == pair) {
                pieces.push(Piece::Filter {
                    hit: Hit::Color(name, hex),
                    words: vec![tok.clone(), next.clone()],
                    explicit: None,
                    ambiguous: true,
                });
                i += 2;
                continue;
            }
        }
        if let Some((hit, ambiguous)) = classify(&lower) {
            pieces.push(Piece::Filter { hit, words: vec![tok.clone()], explicit: None, ambiguous });
        } else {
            let operator = tok.len() > 1
                && (tok.starts_with('@')
                    || tok.starts_with('-')
                    || tok.starts_with('+')
                    || lower.starts_with("like:")
                    || lower.starts_with("id:"));
            if operator {
                let label = match tok.chars().next() {
                    Some('-') => format!("not {}", &tok[1..]),
                    Some('+') => format!("must {}", &tok[1..]),
                    _ => tok.clone(),
                };
                operators.push(Chip {
                    kind: ChipKind::Operator,
                    label,
                    swatch: None,
                    words: vec![tok.clone()],
                    explicit: tok.clone(),
                });
            }
            pieces.push(Piece::Text(tok.clone()));
        }
        i += 1;
    }

    // A query made only of filter words means every one of them.
    let only_filters = pieces.iter().all(|p| matches!(p, Piece::Filter { .. }));
    let mut out = Parsed::default();
    let mut text: Vec<String> = Vec::new();
    let mut purity = [false; 3];
    let mut categories = [false; 3];
    for piece in pieces {
        match piece {
            Piece::Text(t) => text.push(t),
            Piece::Filter { hit, words, explicit, ambiguous } => {
                let explicit_form =
                    explicit.clone().unwrap_or_else(|| explicit_form(hit, &words.join(" ").to_lowercase()));
                if explicit.is_some() || !ambiguous || only_filters {
                    match hit {
                        Hit::Color(_, hex) => out.overlay.color = Some(hex.to_owned()),
                        Hit::Ratio(v, _) => out.overlay.ratios = Some(v.to_owned()),
                        Hit::Resolution(v, _) => out.overlay.atleast = Some(v.to_owned()),
                        Hit::Purity(ix) => purity[ix] = true,
                        Hit::Category(ix) => categories[ix] = true,
                        Hit::Sort(s, _) => out.overlay.sorting = Some(s),
                    }
                    out.chips.push(chip_for(hit, words, explicit_form));
                } else {
                    text.extend(words.iter().cloned());
                    out.suggestions.push(chip_for(hit, words, explicit_form));
                }
            }
        }
    }
    out.chips.extend(operators);
    if purity.iter().any(|p| *p) {
        out.overlay.purity = Some(purity);
    }
    if categories.iter().any(|c| *c) {
        out.overlay.categories = Some(categories);
    }
    out.text = text.join(" ");
    out
}

/// Accept a suggestion: its words become the explicit form.
pub(super) fn promote(query: &str, words: &[String], explicit: &str) -> String {
    let tokens = tokenize(query);
    let Some(start) = tokens.windows(words.len().max(1)).position(|w| w == words) else {
        return query.to_owned();
    };
    let mut out: Vec<String> = tokens[..start].to_vec();
    out.push(explicit.to_owned());
    out.extend_from_slice(&tokens[start + words.len()..]);
    out.join(" ")
}

/// The query with `words` removed (first occurrence of each).
pub(super) fn without(query: &str, words: &[String]) -> String {
    let mut tokens = tokenize(query);
    for w in words {
        if let Some(pos) = tokens.iter().position(|t| t == w) {
            tokens.remove(pos);
        }
    }
    tokens.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{ChipKind, Scope, parse, promote, without};

    #[test]
    fn scope_tokens_round_trip() {
        assert_eq!(Scope::parse("id:101268"), Some(Scope::Tag(101268)));
        assert_eq!(Scope::parse(" like:8g8vgo "), Some(Scope::Like("8g8vgo".into())));
        assert_eq!(Scope::parse("@Izumi123"), Some(Scope::User("Izumi123".into())));
        assert_eq!(Scope::parse("id:12 cats"), None);
        assert_eq!(Scope::parse("cats"), None);
        assert_eq!(Scope::Tag(5).token(), "id:5");
    }
    use crate::wallhaven::Sorting;

    #[test]
    fn ambiguous_words_are_suggested_not_applied() {
        let p = parse("black rock shooter");
        assert_eq!(p.text, "black rock shooter");
        assert!(p.overlay.color.is_none());
        assert_eq!(p.suggestions.len(), 1);
        assert_eq!(p.suggestions[0].explicit, "color:black");
    }

    #[test]
    fn unambiguous_words_apply_alongside_text() {
        let p = parse("dark blue mountains ultrawide 4k");
        assert_eq!(p.text, "dark blue mountains");
        assert!(p.overlay.color.is_none());
        assert_eq!(p.overlay.ratios.as_deref(), Some("21x9,32x9,48x9"));
        assert_eq!(p.overlay.atleast.as_deref(), Some("3840x2160"));
        assert_eq!(p.suggestions[0].label, "dark blue");
    }

    #[test]
    fn filter_only_queries_apply_everything() {
        let p = parse("dark blue 4k anime top");
        assert_eq!(p.text, "");
        assert_eq!(p.overlay.color.as_deref(), Some("333399"));
        assert_eq!(p.overlay.categories, Some([false, true, false]));
        assert_eq!(p.overlay.sorting, Some(Sorting::Toplist));
        assert!(p.suggestions.is_empty());
    }

    #[test]
    fn explicit_forms_always_apply() {
        let p = parse("color:black rock shooter sort:top");
        assert_eq!(p.text, "rock shooter");
        assert_eq!(p.overlay.color.as_deref(), Some("000000"));
        assert_eq!(p.overlay.sorting, Some(Sorting::Toplist));
    }

    #[test]
    fn accepting_a_suggestion_rewrites_it_explicitly() {
        let p = parse("dark blue forest");
        assert!(p.overlay.color.is_none(), "forest is a word, not a color");
        let s = &p.suggestions[0];
        assert_eq!(promote("dark blue forest", &s.words, &s.explicit), "color:dark-blue forest");
        let q = parse("color:dark-blue forest");
        assert_eq!(q.overlay.color.as_deref(), Some("333399"));
        assert_eq!(q.text, "forest");
    }

    #[test]
    fn quotes_keep_words_literal_and_operators_pass_through() {
        let p = parse("\"red dress\" @someone -blurry");
        assert_eq!(p.text, "red dress @someone -blurry");
        assert!(p.overlay.color.is_none() && p.suggestions.is_empty());
        assert_eq!(p.chips.iter().filter(|c| c.kind == ChipKind::Operator).count(), 2);
    }

    #[test]
    fn removing_a_chip_removes_its_words() {
        assert_eq!(without("dark blue forest 4k", &["4k".into()]), "dark blue forest");
    }
}
