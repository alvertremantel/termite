use ratatui::style::{Color, Modifier, Style};

pub fn title() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn badge_root() -> Style {
    Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

pub fn badge_sudo() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

pub fn badge_read_only() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

pub fn caution() -> Style {
    Style::default().fg(Color::LightYellow)
}

pub fn danger() -> Style {
    Style::default()
        .fg(Color::LightRed)
        .add_modifier(Modifier::BOLD)
}

pub fn help() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn sparkline() -> Style {
    Style::default().fg(Color::LightCyan)
}

pub fn empty_bar() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn battery_red() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

pub fn battery_orange() -> Style {
    Style::default()
        .fg(Color::LightRed)
        .add_modifier(Modifier::BOLD)
}

pub fn battery_yellow() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

pub fn battery_lime() -> Style {
    Style::default()
        .fg(Color::LightGreen)
        .add_modifier(Modifier::BOLD)
}

pub fn battery_green() -> Style {
    Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD)
}
