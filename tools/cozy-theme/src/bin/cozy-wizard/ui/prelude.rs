//! What every screen module draws with.
//!
//! A prelude rather than nine hand-maintained import lists: the screens all
//! reach for the same dozen ratatui types, and the interesting part of a screen
//! file should be the layout, not its header.

pub(crate) use cozy_theme::{discover, Adjust, Packages, SLOTS};

pub(crate) use crate::greeting::Greeting;
pub(crate) use crate::icons;
pub(crate) use crate::keys::{Bindings, Key};
pub(crate) use crate::picker::Picker;
pub(crate) use crate::preview;
pub(crate) use crate::resources::{self, Resources, Step};
pub(crate) use crate::theme::{rgb, Theme, SAMPLE_CODE};
pub(crate) use crate::{
    fetch::spawn_fetch, hostcfg::client_config_path, Action, App, Applied, Fetch, FetchKind, Focus,
    Screen, BOX_PADDING_X, BOX_PADDING_Y, DETAIL_ROWS, INPUT_ROWS, INTRO_ROWS, SCHEMES_INTRO_ROWS,
    THEME_INTRO_ROWS,
};
pub(crate) use cozy_theme::Settings as State;

pub(crate) use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub(crate) use ratatui::layout::{Alignment, Constraint, Layout, Rect};
pub(crate) use ratatui::style::{Color, Modifier, Style};
pub(crate) use ratatui::text::{Line, Span, Text};
pub(crate) use ratatui::widgets::{
    Block, BorderType, Borders, List, ListItem, Padding, Paragraph, Wrap,
};
pub(crate) use ratatui::Frame;
pub(crate) use std::path::{Path, PathBuf};
