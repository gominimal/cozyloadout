//! The session-key model: parsing a chord as minimal's client config spells
//! it, and the two validity rules that config load enforces.
//!
//! **This is a port, not an invention.** Every rule here comes from
//! `crates/sessions/src/keys.rs` in the minimal repository, which is what
//! actually reads the config the wizard writes. A wizard that accepted a chord
//! minimal rejects would write a file that fails to load, and one that
//! rejected a chord minimal accepts would be lying about what is possible. The
//! reject sets and the parser range are reproduced verbatim; the comments say
//! why each entry is there so a future reader does not have to guess.
//!
//! Only the parts the wizard needs are ported. The wire-form matching that
//! turns keystrokes into actions lives in the daemon and has no business here:
//! this file writes config, it never interprets a keypress.

use std::fmt;

/// Control bytes the kernel line discipline consumes before the application
/// reads them — the termios `c_cc` set on Linux. A leader bound to one means a
/// leaked or verbatim-forwarded leader triggers kernel behaviour (a signal,
/// flow control, line editing) rather than a minimal binding. See `termios(3)`.
///
/// `ctrl-w` is in here, which is why the greeting's old advice had to change:
/// it is `VWERASE`, and minimal's hard cut to `ctrl-]` retired it.
const TERMIOS_SPECIAL: &[(u8, &str)] = &[
    (0x03, "ctrl-c is SIGINT (VINTR)"),
    (0x04, "ctrl-d is end-of-file (VEOF)"),
    (0x0f, "ctrl-o is VDISCARD"),
    (0x11, "ctrl-q is XON (VSTART)"),
    (0x12, "ctrl-r is VREPRINT"),
    (0x13, "ctrl-s is XOFF (VSTOP)"),
    (0x15, "ctrl-u kills the line (VKILL)"),
    (0x16, "ctrl-v is literal-next (VLNEXT)"),
    (0x17, "ctrl-w erases a word (VWERASE)"),
    (0x1a, "ctrl-z is SIGTSTP (VSUSP)"),
    (0x1c, "ctrl-\\ is SIGQUIT (VQUIT)"),
];

/// Control bytes that alias another commonly-typed key, so binding a leader to
/// one is fragile across terminals. Each entry names the collision so the
/// message can say which key the user would actually be pressing.
const AMBIGUOUS: &[(u8, &str)] = &[
    (0x00, "NUL (also ctrl-Space)"),
    (0x08, "Backspace (the tty erase char is ^H on the BSDs)"),
    (0x09, "Tab"),
    (0x0a, "LF"),
    (0x0d, "Enter (CR)"),
    (0x1b, "Esc"),
];

/// A logical key as config spells it: a base glyph plus an optional Ctrl.
///
/// Canonical forms are `ctrl-]` and `d`. Alt and Shift are not configurable in
/// minimal, so they are not accepted here either.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    codepoint: u32,
    ctrl: bool,
}

/// Why a chord was refused. Rendered straight to the user, so each variant
/// reads as a sentence rather than a code.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum KeyError {
    Empty,
    Unknown(String),
    UnsupportedModifier(String),
    TermiosSpecial(String, &'static str),
    Ambiguous(String, &'static str),
    Shadows(String, &'static str),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "nothing typed yet"),
            Self::Unknown(s) => {
                write!(f, "`{s}` is not a key — try one glyph, or ctrl-<glyph>")
            }
            Self::UnsupportedModifier(s) => {
                write!(f, "`{s}`: only ctrl- is configurable")
            }
            Self::TermiosSpecial(s, why) => write!(f, "`{s}` won't reach minimal — {why}"),
            Self::Ambiguous(s, alias) => write!(f, "`{s}` aliases {alias} on some terminals"),
            Self::Shadows(s, what) => write!(f, "`{s}` shadows {what}"),
        }
    }
}

impl Key {
    /// Parse `ctrl-<glyph>` or a single printable ASCII glyph.
    ///
    /// Ctrl-chords take a glyph in `@`..`~`, because the wire byte is
    /// `codepoint & 0x1f` and that range is what the control-code mapping
    /// covers. Letters normalise to lowercase — `Ctrl+A` and `Ctrl+a` are the
    /// same control code. Plain glyphs take `0x20`..`0x7e` and stay
    /// case-sensitive.
    ///
    /// # Errors
    ///
    /// [`KeyError::UnsupportedModifier`] for `alt-`/`shift-`/`meta-`/`super-`,
    /// [`KeyError::Unknown`] for anything else that is not a key.
    pub fn parse(s: &str) -> Result<Self, KeyError> {
        if s.is_empty() {
            return Err(KeyError::Empty);
        }
        let lower = s.to_ascii_lowercase();
        if let Some(glyph) = lower.strip_prefix("ctrl-") {
            return Self::parse_ctrl(glyph, s);
        }
        for other in ["alt-", "shift-", "meta-", "super-"] {
            if lower.starts_with(other) {
                return Err(KeyError::UnsupportedModifier(s.to_string()));
            }
        }
        let mut chars = s.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return Err(KeyError::Unknown(s.to_string()));
        };
        if ch.is_ascii() && (0x20..=0x7e).contains(&(ch as u32)) {
            Ok(Self {
                codepoint: ch as u32,
                ctrl: false,
            })
        } else {
            Err(KeyError::Unknown(s.to_string()))
        }
    }

    fn parse_ctrl(glyph: &str, original: &str) -> Result<Self, KeyError> {
        let mut chars = glyph.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return Err(KeyError::Unknown(original.to_string()));
        };
        if !ch.is_ascii() || !(0x40..=0x7e).contains(&(ch as u32)) {
            return Err(KeyError::Unknown(original.to_string()));
        }
        Ok(Self {
            codepoint: ch.to_ascii_lowercase() as u32,
            ctrl: true,
        })
    }

    /// The single byte this key puts on the wire: the control code for a
    /// Ctrl-chord, the glyph's own byte otherwise.
    ///
    /// `parse` has already bounded the codepoint to ASCII, so the cast cannot
    /// truncate.
    #[allow(clippy::cast_possible_truncation)]
    fn plain_byte(self) -> u8 {
        if self.ctrl {
            (self.codepoint & 0x1f) as u8
        } else {
            self.codepoint as u8
        }
    }

    /// The canonical config spelling, which is also what the user sees.
    pub fn as_config_str(self) -> String {
        char::from_u32(self.codepoint).map_or_else(String::new, |ch| {
            if self.ctrl {
                format!("ctrl-{ch}")
            } else {
                ch.to_string()
            }
        })
    }

    /// Whether this key is safe to use as the leader.
    ///
    /// Only Ctrl-chords can fail: a plain glyph is a printable byte, never a
    /// control code. A plain leader is a *poor* choice — every press of that
    /// letter enters command mode — but minimal permits it, so this does too,
    /// and the page warns instead of refusing.
    ///
    /// # Errors
    ///
    /// [`KeyError::TermiosSpecial`] or [`KeyError::Ambiguous`].
    pub fn validate_as_leader(self) -> Result<(), KeyError> {
        if !self.ctrl {
            return Ok(());
        }
        let byte = self.plain_byte();
        if let Some((_, why)) = TERMIOS_SPECIAL.iter().find(|(b, _)| *b == byte) {
            return Err(KeyError::TermiosSpecial(self.as_config_str(), why));
        }
        if let Some((_, alias)) = AMBIGUOUS.iter().find(|(b, _)| *b == byte) {
            return Err(KeyError::Ambiguous(self.as_config_str(), alias));
        }
        Ok(())
    }

    /// Whether a plain leader is merely unwise. Not an error — see
    /// [`Self::validate_as_leader`] — but worth saying out loud.
    pub fn is_awkward_leader(self) -> bool {
        !self.ctrl
    }
}

/// The three bindings, as the page holds them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bindings {
    pub leader: Key,
    pub detach: Key,
    pub forward: Key,
    pub bell: bool,
}

impl Default for Bindings {
    fn default() -> Self {
        // minimal's shipped defaults. `forward` defaults *to the leader*, so a
        // double-press forwards a literal leader byte to a nested session.
        let leader = Key::parse("ctrl-]").expect("the shipped leader parses");
        Self {
            leader,
            detach: Key::parse("d").expect("the shipped detach key parses"),
            forward: leader,
            bell: false,
        }
    }
}

impl Bindings {
    /// Catch bindings that shadow each other — what per-key validation cannot
    /// see. Command mode consumes the leader, then checks detach before
    /// forward, so a detach key equal to either makes that binding
    /// unreachable.
    ///
    /// `forward == leader` is deliberately *not* a conflict: it is the shipped
    /// default.
    ///
    /// # Errors
    ///
    /// [`KeyError::Shadows`] naming the binding that would become unreachable.
    pub fn validate(&self) -> Result<(), KeyError> {
        self.leader.validate_as_leader()?;
        if self.detach == self.leader {
            return Err(KeyError::Shadows(
                self.detach.as_config_str(),
                "the leader chord",
            ));
        }
        if self.detach == self.forward {
            return Err(KeyError::Shadows(
                self.detach.as_config_str(),
                "the forward key",
            ));
        }
        Ok(())
    }

    /// The detach gesture as the greeting will print it — the same
    /// `"{leader} then {detach}"` shape minimald builds `MINIMAL_DETACH_HINT`
    /// from.
    pub fn hint(&self) -> String {
        format!(
            "{} then {}",
            self.leader.as_config_str(),
            self.detach.as_config_str()
        )
    }

    /// Whether these are the defaults, i.e. whether writing them would say
    /// anything the absence of a config file does not already say.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> Key {
        Key::parse(s).unwrap_or_else(|e| panic!("{s:?} should parse: {e}"))
    }

    #[test]
    fn parses_the_two_canonical_forms() {
        assert_eq!(key("ctrl-]").as_config_str(), "ctrl-]");
        assert_eq!(key("d").as_config_str(), "d");
    }

    #[test]
    fn ctrl_letters_normalise_to_lowercase() {
        // Ctrl+A and Ctrl+a are one control code; terminals send the lowercase
        // codepoint, so the config form has to agree.
        assert_eq!(key("ctrl-A"), key("ctrl-a"));
        assert_eq!(key("CTRL-A").as_config_str(), "ctrl-a");
    }

    #[test]
    fn plain_glyphs_stay_case_sensitive() {
        assert_ne!(key("d"), key("D"));
    }

    #[test]
    fn rejects_the_things_minimal_rejects() {
        assert!(matches!(Key::parse(""), Err(KeyError::Empty)));
        assert!(matches!(Key::parse("ctrl-"), Err(KeyError::Unknown(_))));
        assert!(matches!(Key::parse("ctrl-foo"), Err(KeyError::Unknown(_))));
        assert!(matches!(Key::parse("esc"), Err(KeyError::Unknown(_))));
        assert!(matches!(Key::parse("é"), Err(KeyError::Unknown(_))));
        // ctrl-<digit> is outside @..~, so the canonical ctrl-@ form is the
        // only way to reach that control code.
        assert!(matches!(Key::parse("ctrl-2"), Err(KeyError::Unknown(_))));
        assert!(matches!(
            Key::parse("alt-x"),
            Err(KeyError::UnsupportedModifier(_))
        ));
        assert!(matches!(
            Key::parse("shift-x"),
            Err(KeyError::UnsupportedModifier(_))
        ));
    }

    #[test]
    fn the_shipped_default_is_a_valid_leader() {
        assert!(Bindings::default().validate().is_ok());
        assert_eq!(Bindings::default().hint(), "ctrl-] then d");
    }

    #[test]
    fn every_termios_special_is_refused_as_a_leader() {
        // The set that made ctrl-w untenable. Each of these is eaten by the
        // line discipline before minimal ever sees it.
        for s in [
            "ctrl-c", "ctrl-d", "ctrl-o", "ctrl-q", "ctrl-r", "ctrl-s", "ctrl-u", "ctrl-v",
            "ctrl-w", "ctrl-z", "ctrl-\\",
        ] {
            assert!(
                matches!(
                    key(s).validate_as_leader(),
                    Err(KeyError::TermiosSpecial(..))
                ),
                "{s} should be refused"
            );
        }
    }

    #[test]
    fn the_old_detach_key_is_now_a_termios_special() {
        // The reason the greeting's advice changed at all.
        let err = key("ctrl-w").validate_as_leader().unwrap_err();
        assert!(err.to_string().contains("VWERASE"), "{err}");
    }

    #[test]
    fn every_ambiguous_alias_is_refused_as_a_leader() {
        for s in ["ctrl-@", "ctrl-h", "ctrl-i", "ctrl-j", "ctrl-m", "ctrl-["] {
            assert!(
                matches!(key(s).validate_as_leader(), Err(KeyError::Ambiguous(..))),
                "{s} should be refused"
            );
        }
    }

    #[test]
    fn a_safe_ctrl_chord_passes() {
        for s in ["ctrl-]", "ctrl-a", "ctrl-b", "ctrl-^", "ctrl-_"] {
            assert!(key(s).validate_as_leader().is_ok(), "{s} should be allowed");
        }
    }

    #[test]
    fn a_plain_leader_is_allowed_but_flagged() {
        // minimal permits it, so refusing here would be the wizard inventing a
        // rule. It is still a bad idea, and the page says so.
        let k = key("d");
        assert!(k.validate_as_leader().is_ok());
        assert!(k.is_awkward_leader());
        assert!(!key("ctrl-]").is_awkward_leader());
    }

    #[test]
    fn a_detach_key_that_shadows_the_leader_is_refused() {
        let leader = key("ctrl-a");
        let b = Bindings {
            leader,
            detach: leader,
            forward: key("x"),
            bell: false,
        };
        assert!(matches!(b.validate(), Err(KeyError::Shadows(_, what)) if what.contains("leader")));
    }

    #[test]
    fn a_detach_key_that_shadows_the_forward_key_is_refused() {
        let b = Bindings {
            leader: key("ctrl-a"),
            detach: key("f"),
            forward: key("f"),
            bell: false,
        };
        assert!(
            matches!(b.validate(), Err(KeyError::Shadows(_, what)) if what.contains("forward"))
        );
    }

    #[test]
    fn forward_equal_to_the_leader_is_the_shipped_default_not_a_conflict() {
        let b = Bindings::default();
        assert_eq!(b.forward, b.leader);
        assert!(b.validate().is_ok());
    }

    #[test]
    fn is_default_tracks_every_field() {
        assert!(Bindings::default().is_default());
        let b = Bindings {
            bell: true,
            ..Bindings::default()
        };
        assert!(!b.is_default(), "the bell flag counts too");
    }
}
