//! The `/` filter, shared by the theme list and the file picker.
//!
//! A thin layer over skim's Sublime-style scorer. Matching a subsequence is
//! easy; *ranking* the matches is the part that decides whether a filter feels
//! right, and that is worth not hand-rolling.
//!
//! No drawing in here — the screens ask which of their rows survive and in what
//! order, and lay them out themselves.

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher as _;
use std::sync::OnceLock;

/// The scorer, built once. Constructing one allocates its scoring tables.
fn matcher() -> &'static SkimMatcherV2 {
    static M: OnceLock<SkimMatcherV2> = OnceLock::new();
    // `ignore_case`, not the default and not smart-case: scheme names and
    // dotfiles are lowercase, so a capital in the query is a slip rather than a
    // request for precision, and smart-case would answer it with nothing.
    M.get_or_init(|| SkimMatcherV2::default().ignore_case())
}

/// The indices of `items` that match `query`, best first.
///
/// An empty query keeps everything **in its original order** — the filter is
/// off, not "everything scored zero", and a list that reshuffled itself the
/// moment you opened the search would be worse than no search.
pub fn filter<T, F>(items: &[T], query: &str, key: F) -> Vec<usize>
where
    F: Fn(&T) -> &str,
{
    if query.trim().is_empty() {
        return (0..items.len()).collect();
    }
    let m = matcher();
    let mut scored: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| m.fuzzy_match(key(item), query).map(|score| (score, i)))
        .collect();
    // Ties broken by original position, so an unchanged query never reorders
    // the list between frames.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: &[&str] = &[
        "gruvbox-dark-hard",
        "gruvbox-light-soft",
        "solarized-dark",
        "nord",
        "tokyo-night",
        "catppuccin-mocha",
    ];

    fn find(query: &str) -> Vec<&'static str> {
        filter(NAMES, query, |s| s)
            .into_iter()
            .map(|i| NAMES[i])
            .collect()
    }

    #[test]
    fn an_empty_query_keeps_everything_in_order() {
        // The filter being off is not the same as everything scoring zero: a
        // list that reshuffled itself the moment you pressed `/` would be worse
        // than having no search at all.
        assert_eq!(find(""), NAMES);
        assert_eq!(find("   "), NAMES);
    }

    #[test]
    fn a_substring_finds_what_contains_it() {
        assert_eq!(find("gruv").len(), 2);
        assert_eq!(find("nord"), vec!["nord"]);
    }

    #[test]
    fn letters_need_not_be_adjacent() {
        // The point of fuzzy: `gdh` should reach `gruvbox-dark-hard`.
        assert!(find("gdh").contains(&"gruvbox-dark-hard"));
        assert!(find("tkn").contains(&"tokyo-night"));
    }

    #[test]
    fn matching_ignores_case() {
        assert_eq!(find("NORD"), vec!["nord"]);
        assert!(find("GruvDark").contains(&"gruvbox-dark-hard"));
    }

    #[test]
    fn nothing_matching_gives_nothing() {
        assert!(find("zzzzzz").is_empty());
    }

    #[test]
    fn the_closer_match_ranks_first() {
        // `dark` is a whole word in one and scattered in the other.
        let hits = find("dark");
        assert_eq!(hits[0], "gruvbox-dark-hard");
    }

    #[test]
    fn the_order_is_stable_for_one_query() {
        // Drawn on every frame, so a ranking that shuffled between identical
        // calls would make the list jitter under the cursor.
        assert_eq!(find("o"), find("o"));
        assert_eq!(find("ar"), find("ar"));
    }

    #[test]
    fn indices_point_back_at_the_original_slice() {
        let idx = filter(NAMES, "nord", |s| s);
        assert_eq!(idx.len(), 1);
        assert_eq!(NAMES[idx[0]], "nord");
    }
}
