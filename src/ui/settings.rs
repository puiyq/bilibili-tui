//! Settings page with theme selection, keybinding display, and account management

use super::{Component, Theme, ThemeChoice, DEFAULT_THEME_ID};
use crate::application::AppAction;
use crate::storage::Keybindings;
use ratatui::{crossterm::event::KeyCode, prelude::*, widgets::*};

/// Settings sections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Theme,
    Keybindings,
    Account,
}

impl SettingsSection {
    pub fn all() -> &'static [SettingsSection] {
        &[
            SettingsSection::Theme,
            SettingsSection::Keybindings,
            SettingsSection::Account,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            SettingsSection::Theme => "🎨 主题",
            SettingsSection::Keybindings => "⌨️ 快捷键",
            SettingsSection::Account => "👤 账户",
        }
    }
}

pub struct SettingsPage {
    pub current_section: SettingsSection,
    pub selected_theme_index: usize,
    pub selected_keybind_index: usize,
    pub keybindings: Keybindings,
    pub current_theme_id: String,
    pub theme_choices: Vec<ThemeChoice>,
    pub is_logged_in: bool,
    section_index: usize,
    pub editing_keybind: bool,
}

impl SettingsPage {
    pub fn new(keybindings: Keybindings, theme_id: String, is_logged_in: bool) -> Self {
        let theme_choices = Theme::available_theme_choices();
        let theme_index = theme_choices
            .iter()
            .position(|t| t.id == theme_id)
            .unwrap_or(0);

        Self {
            current_section: SettingsSection::Theme,
            selected_theme_index: theme_index,
            selected_keybind_index: 0,
            keybindings,
            current_theme_id: theme_id,
            theme_choices,
            is_logged_in,
            section_index: 0,
            editing_keybind: false,
        }
    }

    fn keybind_labels(&self) -> Vec<(&'static str, &str)> {
        vec![
            // Global actions
            ("退出", &self.keybindings.quit),
            ("确认", &self.keybindings.confirm),
            ("返回", &self.keybindings.back),
            ("刷新", &self.keybindings.refresh),
            // Navigation
            ("向上", &self.keybindings.nav_up),
            ("向下", &self.keybindings.nav_down),
            ("向左", &self.keybindings.nav_left),
            ("向右", &self.keybindings.nav_right),
            ("下一页面", &self.keybindings.nav_next_page),
            ("上一页面", &self.keybindings.nav_prev_page),
            // Section/Tab
            ("上一分区", &self.keybindings.section_prev),
            ("下一分区", &self.keybindings.section_next),
            ("标签1", &self.keybindings.tab_1),
            ("标签2", &self.keybindings.tab_2),
            ("标签3", &self.keybindings.tab_3),
            // Actions
            ("切换主题", &self.keybindings.next_theme),
            ("播放", &self.keybindings.play),
            ("设置", &self.keybindings.open_settings),
            ("搜索", &self.keybindings.search_focus),
            // Comments
            ("评论", &self.keybindings.comment),
            ("展开回复", &self.keybindings.toggle_replies),
            // Dynamic page
            ("上一UP", &self.keybindings.up_prev),
            ("下一UP", &self.keybindings.up_next),
        ]
    }
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self::new(Keybindings::default(), DEFAULT_THEME_ID.to_string(), false)
    }
}

impl Component for SettingsPage {
    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme, keys: &Keybindings) {
        // Main layout: header + content
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(10),   // Content
                Constraint::Length(2), // Help
            ])
            .split(area);

        // Header
        let header_line = Line::from(vec![
            Span::styled("⚙️ ", Style::default().fg(theme.bilibili_pink)),
            Span::styled(
                "设置",
                Style::default()
                    .fg(theme.fg_primary)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let header = Paragraph::new(header_line)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_type(BorderType::Plain)
                    .border_style(Style::default().fg(theme.border_subtle)),
            );
        frame.render_widget(header, main_chunks[0]);

        // Content: sidebar + section content
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(16), // Section list
                Constraint::Min(30),    // Section content
            ])
            .split(main_chunks[1]);

        // Section list (sidebar)
        self.draw_section_list(frame, content_chunks[0], theme);

        // Section content
        match self.current_section {
            SettingsSection::Theme => self.draw_theme_section(frame, content_chunks[1], theme),
            SettingsSection::Keybindings => {
                self.draw_keybindings_section(frame, content_chunks[1], theme)
            }
            SettingsSection::Account => self.draw_account_section(frame, content_chunks[1], theme),
        }

        // Help bar
        let help_line = Line::from(vec![
            Span::styled(" [", Style::default().fg(theme.fg_secondary)),
            Span::styled(
                format!("{}{}", keys.section_prev, keys.section_next),
                Style::default()
                    .fg(theme.fg_accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("] ", Style::default().fg(theme.fg_secondary)),
            Span::styled("切换分类", Style::default().fg(theme.fg_secondary)),
            Span::styled("  [", Style::default().fg(theme.fg_secondary)),
            Span::styled(
                format!("{}{}", keys.nav_up, keys.nav_down),
                Style::default()
                    .fg(theme.fg_accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("] ", Style::default().fg(theme.fg_secondary)),
            Span::styled("选择", Style::default().fg(theme.fg_secondary)),
            Span::styled("  [", Style::default().fg(theme.fg_secondary)),
            Span::styled(
                &keys.confirm,
                Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("] ", Style::default().fg(theme.fg_secondary)),
            Span::styled("确认", Style::default().fg(theme.fg_secondary)),
            Span::styled("  [", Style::default().fg(theme.fg_secondary)),
            Span::styled(
                &keys.nav_next_page,
                Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
            ),
            Span::styled("] ", Style::default().fg(theme.fg_secondary)),
            Span::styled("切页面", Style::default().fg(theme.fg_secondary)),
        ]);
        let help = Paragraph::new(help_line).alignment(Alignment::Center);
        frame.render_widget(help, main_chunks[2]);
    }

    fn handle_input(
        &mut self,
        key: KeyCode,
        keys: &crate::storage::Keybindings,
    ) -> Option<AppAction> {
        // Handle keybind editing mode - any key pressed becomes the new binding
        if self.editing_keybind {
            let new_key = crate::storage::Keybindings::keycode_to_string(key);
            self.keybindings
                .update_by_index(self.selected_keybind_index, new_key);
            self.editing_keybind = false;
            // Save keybindings immediately after editing
            return Some(AppAction::SaveKeybindings(Box::new(
                self.keybindings.clone(),
            )));
        }

        if keys.matches_back(key) {
            return Some(AppAction::BackToList);
        }
        if keys.matches_nav_next(key) {
            return Some(AppAction::NavNext);
        }
        if keys.matches_nav_prev(key) {
            return Some(AppAction::NavPrev);
        }
        if keys.matches_section_prev(key) {
            // Cycle through sections backwards
            let sections = SettingsSection::all();
            self.section_index = if self.section_index == 0 {
                sections.len() - 1
            } else {
                self.section_index - 1
            };
            self.current_section = sections[self.section_index];
            return Some(AppAction::None);
        }
        if keys.matches_section_next(key) {
            // Cycle through sections forwards
            let sections = SettingsSection::all();
            self.section_index = (self.section_index + 1) % sections.len();
            self.current_section = sections[self.section_index];
            return Some(AppAction::None);
        }
        if keys.matches_up(key) {
            match self.current_section {
                SettingsSection::Theme => {
                    if self.selected_theme_index > 0 {
                        self.selected_theme_index -= 1;
                    }
                }
                SettingsSection::Keybindings => {
                    if self.selected_keybind_index > 0 {
                        self.selected_keybind_index -= 1;
                    }
                }
                SettingsSection::Account => {}
            }
            return Some(AppAction::None);
        }
        if keys.matches_down(key) {
            match self.current_section {
                SettingsSection::Theme => {
                    let max = self.theme_choices.len().saturating_sub(1);
                    if self.selected_theme_index < max {
                        self.selected_theme_index += 1;
                    }
                }
                SettingsSection::Keybindings => {
                    let max = self.keybindings.get_all_labels().len().saturating_sub(1);
                    if self.selected_keybind_index < max {
                        self.selected_keybind_index += 1;
                    }
                }
                SettingsSection::Account => {}
            }
            return Some(AppAction::None);
        }
        if keys.matches_confirm(key) {
            match self.current_section {
                SettingsSection::Theme => {
                    if let Some(selected) = self.theme_choices.get(self.selected_theme_index) {
                        self.current_theme_id = selected.id.clone();
                        return Some(AppAction::SetTheme(selected.id.clone()));
                    }
                }
                SettingsSection::Account => {
                    return Some(if self.is_logged_in {
                        AppAction::Logout
                    } else {
                        AppAction::SwitchToLogin
                    });
                }
                SettingsSection::Keybindings => {
                    // Enter keybind editing mode
                    self.editing_keybind = true;
                }
            }
            return Some(AppAction::None);
        }
        if keys.matches_quit(key) {
            return Some(AppAction::Quit);
        }
        Some(AppAction::None)
    }
}

impl SettingsPage {
    fn draw_section_list(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::RIGHT)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(theme.border_subtle))
            .title(Span::styled(
                " 分类 ",
                Style::default()
                    .fg(theme.bilibili_pink)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: Vec<ListItem> = SettingsSection::all()
            .iter()
            .map(|section| {
                let is_selected = *section == self.current_section;
                let style = if is_selected {
                    Style::default()
                        .fg(theme.fg_accent)
                        .add_modifier(Modifier::BOLD)
                        .bg(theme.selection_bg)
                } else {
                    Style::default().fg(theme.fg_secondary)
                };

                let prefix = if is_selected { "▶ " } else { "  " };
                ListItem::new(format!("{}{}", prefix, section.label())).style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn draw_theme_section(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_subtle))
            .title(Span::styled(
                " 🎨 选择主题 ",
                Style::default()
                    .fg(theme.bilibili_pink)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: Vec<ListItem> = self
            .theme_choices
            .iter()
            .enumerate()
            .map(|(idx, choice)| {
                let is_selected = idx == self.selected_theme_index;
                let is_current = choice.id == self.current_theme_id;

                let mut style = if is_selected {
                    Style::default()
                        .fg(theme.fg_primary)
                        .add_modifier(Modifier::BOLD)
                        .bg(theme.selection_bg)
                } else {
                    Style::default().fg(theme.fg_secondary)
                };

                let prefix = if is_selected { "▶ " } else { "  " };
                let suffix = if is_current { " ✓" } else { "" };

                if is_current && !is_selected {
                    style = style.fg(theme.success);
                }

                ListItem::new(format!("{}{}{}", prefix, choice.label.as_str(), suffix)).style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn draw_keybindings_section(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_subtle))
            .title(Span::styled(
                " ⌨️ 快捷键 ",
                Style::default()
                    .fg(theme.bilibili_pink)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let labels = self.keybind_labels();
        let items: Vec<ListItem> = labels
            .iter()
            .enumerate()
            .map(|(idx, (label, key))| {
                let is_selected = idx == self.selected_keybind_index;
                let style = if is_selected {
                    Style::default()
                        .fg(theme.fg_primary)
                        .add_modifier(Modifier::BOLD)
                        .bg(theme.selection_bg)
                } else {
                    Style::default().fg(theme.fg_secondary)
                };

                let prefix = if is_selected { "▶ " } else { "  " };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(format!("{:<12}", label), style),
                    Span::styled(
                        format!("[{}]", key),
                        Style::default()
                            .fg(theme.fg_accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn draw_account_section(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_subtle))
            .title(Span::styled(
                " 👤 账户 ",
                Style::default()
                    .fg(theme.bilibili_pink)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Layout for account info + action button
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3), // Info
                Constraint::Length(3), // Action button
                Constraint::Min(0),    // Spacer
            ])
            .split(inner);

        let info = Paragraph::new(if self.is_logged_in {
            "已登录"
        } else {
            "未登录"
        })
        .style(Style::default().fg(if self.is_logged_in {
            theme.success
        } else {
            theme.warning
        }))
        .alignment(Alignment::Left);
        frame.render_widget(info, chunks[0]);

        let action_label = if self.is_logged_in {
            "▶ 退出登录"
        } else {
            "▶ 去登录"
        };
        let action_color = if self.is_logged_in {
            theme.error
        } else {
            theme.info
        };
        let action_btn = Paragraph::new(action_label)
            .style(
                Style::default()
                    .fg(action_color)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(action_color)),
            )
            .alignment(Alignment::Center);
        frame.render_widget(action_btn, chunks[1]);
    }
}
