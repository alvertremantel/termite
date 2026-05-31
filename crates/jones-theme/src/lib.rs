use ratatui::style::{Color, Modifier, Style};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const DEFAULT_THEME_ID: &str = "space";

static CURRENT_THEME: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    palette: Palette,
    backgrounds: Backgrounds,
}

#[derive(Debug, Clone, Copy)]
struct Palette {
    text_primary: Color,
    text_secondary: Color,
    text_muted: Color,
    text_bright: Color,
    text_dim: Color,
    accent_blue: Color,
    accent_blue_dim: Color,
    accent_blue_bright: Color,
    accent_yellow: Color,
    accent_yellow_dim: Color,
    accent_yellow_bright: Color,
    accent_green: Color,
    accent_magenta: Color,
    accent_cyan: Color,
    accent_cyan_rgb: Color,
    accent_orange: Color,
    border_focused: Color,
    border_unfocused: Color,
    link: Color,
    code_fg: Color,
    code_block_fg: Color,
    heading_h1: Color,
    heading_h2: Color,
    heading_h3: Color,
    heading_h4: Color,
    heading_h5: Color,
    heading_h6: Color,
    status_fg: Color,
    status_badge_fg: Color,
    notify_info_fg: Color,
    notify_success_fg: Color,
    notify_error_fg: Color,
    inactive_tab: Color,
}

#[derive(Debug, Clone, Copy)]
struct Backgrounds {
    dark: Color,
    surface: Color,
    highlight: Color,
    active: Color,
    code: Color,
    code_block: Color,
    status: Color,
    status_badge: Color,
    search_match: Color,
    search_current: Color,
    selection: Color,
    notify_info: Color,
    notify_success: Color,
    notify_error: Color,
}

impl Theme {
    const fn new(
        id: &'static str,
        name: &'static str,
        description: &'static str,
        palette: Palette,
        backgrounds: Backgrounds,
    ) -> Self {
        Self {
            id,
            name,
            description,
            palette,
            backgrounds,
        }
    }
}

impl Backgrounds {
    const fn uniform(color: Color) -> Self {
        Self {
            dark: color,
            surface: color,
            highlight: color,
            active: color,
            code: color,
            code_block: color,
            status: color,
            status_badge: color,
            search_match: color,
            search_current: color,
            selection: color,
            notify_info: color,
            notify_success: color,
            notify_error: color,
        }
    }
}

const SPACE_PALETTE: Palette = Palette {
    text_primary: Color::Rgb(200, 210, 220),
    text_secondary: Color::Rgb(120, 135, 160),
    text_muted: Color::Rgb(60, 72, 92),
    text_bright: Color::Rgb(230, 238, 250),
    text_dim: Color::Rgb(90, 90, 100),
    accent_blue: Color::Rgb(110, 180, 240),
    accent_blue_dim: Color::Rgb(55, 95, 150),
    accent_blue_bright: Color::Rgb(140, 200, 255),
    accent_yellow: Color::Rgb(230, 210, 130),
    accent_yellow_dim: Color::Rgb(150, 138, 75),
    accent_yellow_bright: Color::Rgb(255, 235, 150),
    accent_green: Color::Rgb(100, 200, 140),
    accent_magenta: Color::Rgb(180, 140, 220),
    accent_cyan: Color::Cyan,
    accent_cyan_rgb: Color::Rgb(100, 210, 255),
    accent_orange: Color::Rgb(220, 150, 120),
    border_focused: Color::Rgb(110, 180, 240),
    border_unfocused: Color::Rgb(35, 42, 58),
    link: Color::Rgb(80, 160, 255),
    code_fg: Color::Rgb(220, 200, 120),
    code_block_fg: Color::Rgb(220, 200, 120),
    heading_h1: Color::Rgb(100, 210, 255),
    heading_h2: Color::Rgb(130, 220, 130),
    heading_h3: Color::Rgb(240, 210, 100),
    heading_h4: Color::Rgb(200, 160, 240),
    heading_h5: Color::Rgb(220, 150, 120),
    heading_h6: Color::Rgb(160, 180, 200),
    status_fg: Color::Rgb(180, 185, 200),
    status_badge_fg: Color::Rgb(10, 12, 18),
    notify_info_fg: Color::Rgb(130, 180, 240),
    notify_success_fg: Color::Rgb(100, 220, 100),
    notify_error_fg: Color::Rgb(240, 100, 100),
    inactive_tab: Color::Rgb(100, 105, 120),
};

const CLEAN_BLUE_PALETTE: Palette = Palette {
    text_primary: Color::Rgb(205, 224, 242),
    text_secondary: Color::Rgb(132, 166, 198),
    text_muted: Color::Rgb(78, 110, 142),
    text_bright: Color::Rgb(236, 247, 255),
    text_dim: Color::Rgb(92, 118, 145),
    accent_blue: Color::Rgb(94, 177, 245),
    accent_blue_dim: Color::Rgb(62, 128, 184),
    accent_blue_bright: Color::Rgb(144, 211, 255),
    accent_yellow: Color::Rgb(230, 214, 146),
    accent_yellow_dim: Color::Rgb(164, 145, 90),
    accent_yellow_bright: Color::Rgb(255, 236, 166),
    accent_green: Color::Rgb(118, 214, 162),
    accent_magenta: Color::Rgb(184, 154, 232),
    accent_cyan: Color::Rgb(104, 218, 255),
    accent_cyan_rgb: Color::Rgb(104, 218, 255),
    accent_orange: Color::Rgb(230, 164, 128),
    border_focused: Color::Rgb(120, 200, 255),
    border_unfocused: Color::Rgb(52, 96, 132),
    link: Color::Rgb(120, 200, 255),
    code_fg: Color::Rgb(230, 214, 146),
    code_block_fg: Color::Rgb(230, 214, 146),
    heading_h1: Color::Rgb(104, 218, 255),
    heading_h2: Color::Rgb(118, 214, 162),
    heading_h3: Color::Rgb(230, 214, 146),
    heading_h4: Color::Rgb(184, 154, 232),
    heading_h5: Color::Rgb(230, 164, 128),
    heading_h6: Color::Rgb(132, 166, 198),
    status_fg: Color::Rgb(132, 166, 198),
    status_badge_fg: Color::Rgb(144, 211, 255),
    notify_info_fg: Color::Rgb(120, 200, 255),
    notify_success_fg: Color::Rgb(118, 214, 162),
    notify_error_fg: Color::Rgb(250, 122, 122),
    inactive_tab: Color::Rgb(78, 110, 142),
};

const SPACE_BACKGROUNDS: Backgrounds = Backgrounds {
    dark: Color::Rgb(10, 12, 18),
    surface: Color::Rgb(18, 22, 32),
    highlight: Color::Rgb(25, 32, 48),
    active: Color::Rgb(32, 42, 62),
    code: Color::Rgb(35, 35, 45),
    code_block: Color::Rgb(30, 30, 40),
    status: Color::Rgb(30, 32, 42),
    status_badge: Color::Rgb(110, 180, 240),
    search_match: Color::Rgb(100, 90, 30),
    search_current: Color::Rgb(180, 160, 40),
    selection: Color::Rgb(55, 65, 100),
    notify_info: Color::Rgb(25, 30, 50),
    notify_success: Color::Rgb(25, 45, 25),
    notify_error: Color::Rgb(50, 25, 25),
};

const TRANSPARENT_BACKGROUNDS: Backgrounds = Backgrounds::uniform(Color::Reset);

const SPACE: Theme = Theme::new(
    DEFAULT_THEME_ID,
    "Deep Space",
    "Original dark blue theme with painted surfaces.",
    SPACE_PALETTE,
    SPACE_BACKGROUNDS,
);

const CLEAN_BLUE: Theme = Theme::new(
    "clean-blue",
    "Clean Blue",
    "Transparent, background-free blue theme for blurred terminals.",
    CLEAN_BLUE_PALETTE,
    TRANSPARENT_BACKGROUNDS,
);

// Add new themes by defining a Palette and Backgrounds above, then adding the
// Theme here. Existing draw code should only need a new accessor when it needs a
// brand-new semantic color role.
pub const THEMES: &[Theme] = &[SPACE, CLEAN_BLUE];

pub const BG_DARK: Color = SPACE_BACKGROUNDS.dark;
pub const BG_SURFACE: Color = SPACE_BACKGROUNDS.surface;
pub const BG_HIGHLIGHT: Color = SPACE_BACKGROUNDS.highlight;
pub const BG_ACTIVE: Color = SPACE_BACKGROUNDS.active;
pub const TEXT_PRIMARY: Color = SPACE_PALETTE.text_primary;
pub const TEXT_SECONDARY: Color = SPACE_PALETTE.text_secondary;
pub const TEXT_MUTED: Color = SPACE_PALETTE.text_muted;
pub const TEXT_BRIGHT: Color = SPACE_PALETTE.text_bright;
pub const TEXT_DIM: Color = SPACE_PALETTE.text_dim;
pub const ACCENT_BLUE: Color = SPACE_PALETTE.accent_blue;
pub const ACCENT_BLUE_DIM: Color = SPACE_PALETTE.accent_blue_dim;
pub const ACCENT_BLUE_BRIGHT: Color = SPACE_PALETTE.accent_blue_bright;
pub const ACCENT_YELLOW: Color = SPACE_PALETTE.accent_yellow;
pub const ACCENT_YELLOW_DIM: Color = SPACE_PALETTE.accent_yellow_dim;
pub const ACCENT_YELLOW_BRIGHT: Color = SPACE_PALETTE.accent_yellow_bright;
pub const ACCENT_GREEN: Color = SPACE_PALETTE.accent_green;
pub const ACCENT_MAGENTA: Color = SPACE_PALETTE.accent_magenta;
pub const ACCENT_CYAN: Color = SPACE_PALETTE.accent_cyan;
pub const ACCENT_CYAN_RGB: Color = SPACE_PALETTE.accent_cyan_rgb;
pub const ACCENT_ORANGE: Color = SPACE_PALETTE.accent_orange;
pub const BORDER_FOCUSED: Color = SPACE_PALETTE.border_focused;
pub const BORDER_UNFOCUSED: Color = SPACE_PALETTE.border_unfocused;
pub const RSS_ACCENT: Color = SPACE_PALETTE.accent_blue;
pub const MD_ACCENT: Color = SPACE_PALETTE.accent_cyan;
pub const UNFOCUSED: Color = SPACE_PALETTE.text_muted;
pub const LINK: Color = SPACE_PALETTE.link;
pub const HIGHLIGHT: Color = SPACE_PALETTE.accent_yellow;
pub const CODE_FG: Color = SPACE_PALETTE.code_fg;
pub const CODE_BG: Color = SPACE_BACKGROUNDS.code;
pub const CODE_BLOCK_FG: Color = SPACE_PALETTE.code_block_fg;
pub const CODE_BLOCK_BG: Color = SPACE_BACKGROUNDS.code_block;
pub const HEADING_H1: Color = SPACE_PALETTE.heading_h1;
pub const HEADING_H2: Color = SPACE_PALETTE.heading_h2;
pub const HEADING_H3: Color = SPACE_PALETTE.heading_h3;
pub const HEADING_H4: Color = SPACE_PALETTE.heading_h4;
pub const HEADING_H5: Color = SPACE_PALETTE.heading_h5;
pub const HEADING_H6: Color = SPACE_PALETTE.heading_h6;
pub const HEADING_DEFAULT: Color = SPACE_PALETTE.text_primary;
pub const LIST_BULLET: Color = SPACE_PALETTE.accent_cyan;
pub const TASK_MARKER: Color = SPACE_PALETTE.accent_cyan;
pub const BLOCKQUOTE: Color = SPACE_PALETTE.text_dim;
pub const RULE: Color = SPACE_PALETTE.text_dim;
pub const IMAGE_TAG: Color = SPACE_PALETTE.accent_magenta;
pub const FOOTNOTE_REF: Color = SPACE_PALETTE.accent_cyan_rgb;
pub const FOOTNOTE_DEF: Color = SPACE_PALETTE.text_secondary;
pub const TABLE_BORDER: Color = SPACE_PALETTE.text_dim;
pub const TABLE_HEADER: Color = SPACE_PALETTE.accent_blue;
pub const CODE_LANG_LABEL: Color = SPACE_PALETTE.text_secondary;
pub const STATUS_BG: Color = SPACE_BACKGROUNDS.status;
pub const STATUS_FG: Color = SPACE_PALETTE.status_fg;
pub const STATUS_BADGE_BG: Color = SPACE_BACKGROUNDS.status_badge;
pub const STATUS_BADGE_FG: Color = SPACE_PALETTE.status_badge_fg;
pub const READ_ARTICLE: Color = SPACE_PALETTE.text_muted;
pub const UNREAD_ARTICLE: Color = SPACE_PALETTE.text_primary;
pub const STAR: Color = SPACE_PALETTE.accent_yellow_bright;
pub const UNREAD_COUNT: Color = SPACE_PALETTE.accent_yellow;
pub const ARTICLE_TITLE: Color = SPACE_PALETTE.accent_blue;
pub const SEPARATOR: Color = SPACE_PALETTE.border_unfocused;
pub const SECTION_HEADER: Color = SPACE_PALETTE.text_secondary;
pub const SECTION_DIVIDER: Color = SPACE_PALETTE.border_unfocused;
pub const DIR_COLOR: Color = SPACE_PALETTE.accent_cyan;
pub const FILE_COLOR: Color = SPACE_PALETTE.text_primary;
pub const SEARCH_BORDER: Color = SPACE_PALETTE.accent_yellow;
pub const SEARCH_PROMPT: Color = SPACE_PALETTE.accent_yellow;
pub const SEARCH_MATCH_BG: Color = SPACE_BACKGROUNDS.search_match;
pub const SEARCH_CURRENT_BG: Color = SPACE_BACKGROUNDS.search_current;
pub const HELP_BORDER: Color = SPACE_PALETTE.accent_cyan;
pub const HELP_HEADING: Color = SPACE_PALETTE.accent_cyan;
pub const HELP_KEY: Color = SPACE_PALETTE.accent_yellow;
pub const SELECTION_BG: Color = SPACE_BACKGROUNDS.selection;
pub const EDITOR_GUTTER: Color = SPACE_PALETTE.text_dim;
pub const EDITOR_GUTTER_ACTIVE: Color = SPACE_PALETTE.accent_cyan;
pub const NOTIFY_INFO_FG: Color = SPACE_PALETTE.notify_info_fg;
pub const NOTIFY_INFO_BG: Color = SPACE_BACKGROUNDS.notify_info;
pub const NOTIFY_SUCCESS_FG: Color = SPACE_PALETTE.notify_success_fg;
pub const NOTIFY_SUCCESS_BG: Color = SPACE_BACKGROUNDS.notify_success;
pub const NOTIFY_ERROR_FG: Color = SPACE_PALETTE.notify_error_fg;
pub const NOTIFY_ERROR_BG: Color = SPACE_BACKGROUNDS.notify_error;
pub const ACTIVE_TAB: Color = SPACE_PALETTE.accent_cyan;
pub const INACTIVE_TAB: Color = SPACE_PALETTE.inactive_tab;

pub fn available() -> &'static [Theme] {
    THEMES
}

pub fn current() -> &'static Theme {
    &THEMES[CURRENT_THEME.load(Ordering::Relaxed).min(THEMES.len() - 1)]
}

pub fn find(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|theme| theme.id == id)
}

pub fn set_current(id: &str) -> &'static Theme {
    let index = THEMES.iter().position(|theme| theme.id == id).unwrap_or(0);
    CURRENT_THEME.store(index, Ordering::Relaxed);
    &THEMES[index]
}

pub fn next_id(id: &str) -> &'static str {
    let index = THEMES.iter().position(|theme| theme.id == id).unwrap_or(0);
    THEMES[(index + 1) % THEMES.len()].id
}

macro_rules! theme_color {
    ($($name:ident => $expr:expr),+ $(,)?) => {
        $(pub fn $name() -> Color { ($expr)(current()) })+
    };
}

theme_color! {
    bg_dark => |theme: &Theme| theme.backgrounds.dark,
    bg_surface => |theme: &Theme| theme.backgrounds.surface,
    bg_highlight => |theme: &Theme| theme.backgrounds.highlight,
    bg_active => |theme: &Theme| theme.backgrounds.active,
    text_primary => |theme: &Theme| theme.palette.text_primary,
    text_secondary => |theme: &Theme| theme.palette.text_secondary,
    text_muted => |theme: &Theme| theme.palette.text_muted,
    text_bright => |theme: &Theme| theme.palette.text_bright,
    text_dim => |theme: &Theme| theme.palette.text_dim,
    accent_blue => |theme: &Theme| theme.palette.accent_blue,
    accent_blue_dim => |theme: &Theme| theme.palette.accent_blue_dim,
    accent_blue_bright => |theme: &Theme| theme.palette.accent_blue_bright,
    accent_yellow => |theme: &Theme| theme.palette.accent_yellow,
    accent_yellow_dim => |theme: &Theme| theme.palette.accent_yellow_dim,
    accent_yellow_bright => |theme: &Theme| theme.palette.accent_yellow_bright,
    accent_green => |theme: &Theme| theme.palette.accent_green,
    accent_magenta => |theme: &Theme| theme.palette.accent_magenta,
    accent_cyan => |theme: &Theme| theme.palette.accent_cyan,
    accent_cyan_rgb => |theme: &Theme| theme.palette.accent_cyan_rgb,
    accent_orange => |theme: &Theme| theme.palette.accent_orange,
    border_focused => |theme: &Theme| theme.palette.border_focused,
    border_unfocused => |theme: &Theme| theme.palette.border_unfocused,
    rss_accent => |theme: &Theme| theme.palette.accent_blue,
    md_accent => |theme: &Theme| theme.palette.accent_cyan,
    unfocused => |theme: &Theme| theme.palette.text_muted,
    link => |theme: &Theme| theme.palette.link,
    highlight => |theme: &Theme| theme.palette.accent_yellow,
    code_fg => |theme: &Theme| theme.palette.code_fg,
    code_bg => |theme: &Theme| theme.backgrounds.code,
    code_block_fg => |theme: &Theme| theme.palette.code_block_fg,
    code_block_bg => |theme: &Theme| theme.backgrounds.code_block,
    heading_h1 => |theme: &Theme| theme.palette.heading_h1,
    heading_h2 => |theme: &Theme| theme.palette.heading_h2,
    heading_h3 => |theme: &Theme| theme.palette.heading_h3,
    heading_h4 => |theme: &Theme| theme.palette.heading_h4,
    heading_h5 => |theme: &Theme| theme.palette.heading_h5,
    heading_h6 => |theme: &Theme| theme.palette.heading_h6,
    heading_default => |theme: &Theme| theme.palette.text_primary,
    list_bullet => |theme: &Theme| theme.palette.accent_cyan,
    task_marker => |theme: &Theme| theme.palette.accent_cyan,
    blockquote => |theme: &Theme| theme.palette.text_dim,
    rule => |theme: &Theme| theme.palette.text_dim,
    image_tag => |theme: &Theme| theme.palette.accent_magenta,
    footnote_ref => |theme: &Theme| theme.palette.accent_cyan_rgb,
    footnote_def => |theme: &Theme| theme.palette.text_secondary,
    table_border => |theme: &Theme| theme.palette.text_dim,
    table_header => |theme: &Theme| theme.palette.accent_blue,
    code_lang_label => |theme: &Theme| theme.palette.text_secondary,
    status_bg => |theme: &Theme| theme.backgrounds.status,
    status_fg => |theme: &Theme| theme.palette.status_fg,
    status_badge_bg => |theme: &Theme| theme.backgrounds.status_badge,
    status_badge_fg => |theme: &Theme| theme.palette.status_badge_fg,
    read_article => |theme: &Theme| theme.palette.text_muted,
    unread_article => |theme: &Theme| theme.palette.text_primary,
    star => |theme: &Theme| theme.palette.accent_yellow_bright,
    unread_count => |theme: &Theme| theme.palette.accent_yellow,
    article_title => |theme: &Theme| theme.palette.accent_blue,
    separator => |theme: &Theme| theme.palette.border_unfocused,
    section_header => |theme: &Theme| theme.palette.text_secondary,
    section_divider => |theme: &Theme| theme.palette.border_unfocused,
    dir_color => |theme: &Theme| theme.palette.accent_cyan,
    file_color => |theme: &Theme| theme.palette.text_primary,
    search_border => |theme: &Theme| theme.palette.accent_yellow,
    search_prompt => |theme: &Theme| theme.palette.accent_yellow,
    search_match_bg => |theme: &Theme| theme.backgrounds.search_match,
    search_current_bg => |theme: &Theme| theme.backgrounds.search_current,
    help_border => |theme: &Theme| theme.palette.accent_cyan,
    help_heading => |theme: &Theme| theme.palette.accent_cyan,
    help_key => |theme: &Theme| theme.palette.accent_yellow,
    selection_bg => |theme: &Theme| theme.backgrounds.selection,
    editor_gutter => |theme: &Theme| theme.palette.text_dim,
    editor_gutter_active => |theme: &Theme| theme.palette.accent_cyan,
    notify_info_fg => |theme: &Theme| theme.palette.notify_info_fg,
    notify_info_bg => |theme: &Theme| theme.backgrounds.notify_info,
    notify_success_fg => |theme: &Theme| theme.palette.notify_success_fg,
    notify_success_bg => |theme: &Theme| theme.backgrounds.notify_success,
    notify_error_fg => |theme: &Theme| theme.palette.notify_error_fg,
    notify_error_bg => |theme: &Theme| theme.backgrounds.notify_error,
    active_tab => |theme: &Theme| theme.palette.accent_cyan,
    inactive_tab => |theme: &Theme| theme.palette.inactive_tab,
}

/// Return the appropriate style for a tab button based on its active state.
pub fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(active_tab())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(inactive_tab())
    }
}

/// Base style for the application background.
pub fn base_style() -> Style {
    Style::default().bg(bg_dark()).fg(text_primary())
}

/// Style for the status bar.
pub fn status_bar_style() -> Style {
    Style::default().bg(status_bg()).fg(status_fg())
}

/// Style for dimmed / secondary text.
pub fn dim_style() -> Style {
    Style::default().fg(text_dim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn theme_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn default_theme_is_available_and_selected() {
        let _guard = theme_lock().lock().unwrap();
        let theme = set_current(DEFAULT_THEME_ID);
        assert_eq!(theme.id, DEFAULT_THEME_ID);
        assert_eq!(current().id, DEFAULT_THEME_ID);
        assert!(
            available()
                .iter()
                .any(|candidate| candidate.id == DEFAULT_THEME_ID)
        );
    }

    #[test]
    fn next_theme_cycles_back_to_default() {
        let _guard = theme_lock().lock().unwrap();
        set_current(DEFAULT_THEME_ID);

        let mut visited = Vec::new();
        let mut id = DEFAULT_THEME_ID;
        loop {
            visited.push(id);
            id = next_id(id);
            if id == DEFAULT_THEME_ID {
                break;
            }
        }

        assert_eq!(visited.len(), THEMES.len());
        assert_eq!(id, DEFAULT_THEME_ID);
    }

    #[test]
    fn theme_ids_are_unique() {
        let mut ids = THEMES.iter().map(|theme| theme.id).collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), THEMES.len());
    }

    #[test]
    fn uppercase_constants_match_default_theme_accessors() {
        let _guard = theme_lock().lock().unwrap();
        set_current(DEFAULT_THEME_ID);

        assert_eq!(BG_DARK, bg_dark());
        assert_eq!(BG_SURFACE, bg_surface());
        assert_eq!(TEXT_PRIMARY, text_primary());
        assert_eq!(TEXT_SECONDARY, text_secondary());
        assert_eq!(ACCENT_CYAN, accent_cyan());
        assert_eq!(LINK, link());
        assert_eq!(STATUS_BG, status_bg());
        assert_eq!(STATUS_BADGE_FG, status_badge_fg());
    }
}
