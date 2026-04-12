//! Application-wide theming based on the GtkSourceView style scheme.
//!
//! When the selected sourceview theme has a custom background, the entire
//! application chrome adopts a derived palette so the UI feels cohesive.
//! The system's dark/light preference is also set to match.

/// Terminal color palette extracted from the current theme.
#[derive(Debug, Clone)]
pub struct TerminalColors {
    /// Terminal background color.
    pub background: gtk4::gdk::RGBA,
    /// Terminal foreground (text) color.
    pub foreground: gtk4::gdk::RGBA,
    /// Selection highlight background color.
    pub highlight: gtk4::gdk::RGBA,
    /// Selection highlight foreground color.
    pub highlight_foreground: gtk4::gdk::RGBA,
    /// Bold text color.
    pub bold: gtk4::gdk::RGBA,
}

/// Extract terminal-appropriate colors from a sourceview style scheme.
///
/// Returns `None` if the scheme cannot be found or has no background color.
pub fn terminal_colors_for_scheme(scheme_id: &str) -> Option<TerminalColors> {
    let scheme_manager = sourceview5::StyleSchemeManager::default();
    let scheme = scheme_manager.scheme(scheme_id)?;

    let syntax_theme = rline_config::SyntaxTheme::load(scheme_id).ok().flatten();

    let ui = |key: &str| -> Option<String> {
        syntax_theme
            .as_ref()
            .and_then(|t| t.ui_color(key))
            .map(|s| s.to_string())
    };

    // Editor background
    let bg_hex = scheme
        .style("text")
        .and_then(|style| {
            if style.is_background_set() {
                style.background()
            } else {
                None
            }
        })
        .map(|s| s.to_string())?;

    let fg_hex = scheme
        .style("text")
        .and_then(|style| {
            if style.is_foreground_set() {
                style.foreground()
            } else {
                None
            }
        })
        .map(|s| s.to_string());

    let is_dark = perceived_brightness(&bg_hex) < 128;

    let terminal_bg = ui("terminal.background").unwrap_or_else(|| bg_hex.clone());
    let terminal_fg = ui("terminal.foreground").unwrap_or_else(|| {
        fg_hex
            .clone()
            .unwrap_or_else(|| if is_dark { "#e0e0e0" } else { "#1e1e1e" }.into())
    });

    let highlight_hex = ui("terminal.selectionBackground").unwrap_or_else(|| {
        if is_dark {
            "#264f78".into()
        } else {
            "#add6ff".into()
        }
    });
    let highlight_fg_hex =
        ui("terminal.selectionForeground").unwrap_or_else(|| terminal_fg.clone());

    let bold_hex = ui("terminal.ansiBrightWhite").unwrap_or_else(|| {
        if is_dark {
            "#ffffff".into()
        } else {
            "#000000".into()
        }
    });

    let parse = |hex: &str| -> gtk4::gdk::RGBA {
        gtk4::gdk::RGBA::parse(hex).unwrap_or(gtk4::gdk::RGBA::WHITE)
    };

    Some(TerminalColors {
        background: parse(&terminal_bg),
        foreground: parse(&terminal_fg),
        highlight: parse(&highlight_hex),
        highlight_foreground: parse(&highlight_fg_hex),
        bold: parse(&bold_hex),
    })
}

/// Apply application-wide theming derived from the given sourceview style scheme.
///
/// Extracts the editor background color, derives a chrome palette from it,
/// sets the GTK dark theme preference accordingly, and applies global CSS
/// covering all UI elements.
/// Apply font rendering settings (hinting, antialiasing).
///
/// `hint_style` should be `"full"` or `"slight"`. Full hinting snaps glyphs
/// to the pixel grid for maximum crispness; slight hinting preserves glyph
/// shapes at the cost of some softness.
pub fn apply_font_rendering(hint_style: &str) {
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_font_rendering(gtk4::FontRendering::Manual);
        settings.set_gtk_hint_font_metrics(true);
        settings.set_gtk_xft_antialias(1);
        settings.set_gtk_xft_hinting(1);
        let style = match hint_style {
            "slight" => "hintslight",
            _ => "hintfull",
        };
        settings.set_gtk_xft_hintstyle(Some(style));
    }
}

pub fn apply_app_theme(scheme_id: &str) {
    let scheme_manager = sourceview5::StyleSchemeManager::default();
    let scheme = match scheme_manager.scheme(scheme_id) {
        Some(s) => s,
        None => return,
    };

    // Try to load the rich syntax theme for VS Code UI colors
    let syntax_theme = match rline_config::SyntaxTheme::load(scheme_id) {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!("no syntax theme for {scheme_id}: {e}");
            None
        }
    };

    // Get the background color from the "text" style (the editor background)
    let bg_color = scheme
        .style("text")
        .and_then(|style| {
            if style.is_background_set() {
                style.background()
            } else {
                None
            }
        })
        .map(|s| s.to_string());

    // Get the foreground color for text
    let fg_color = scheme
        .style("text")
        .and_then(|style| {
            if style.is_foreground_set() {
                style.foreground()
            } else {
                None
            }
        })
        .map(|s| s.to_string());

    let css = match bg_color {
        Some(ref bg) => {
            let is_dark = perceived_brightness(bg) < 128;

            // Tell GTK to use dark or light window decorations
            if let Some(settings) = gtk4::Settings::default() {
                settings.set_gtk_application_prefer_dark_theme(is_dark);
            }

            // Use VS Code UI colors when available, fall back to derived values
            let ui = |key: &str| {
                syntax_theme
                    .as_ref()
                    .and_then(|t| t.ui_color(key))
                    .map(|s| s.to_string())
            };

            let editor_bg = bg.clone();
            let chrome = ui("sideBar.background").unwrap_or_else(|| darken_color(bg, 0.85));
            let chrome_darker =
                ui("activityBar.background").unwrap_or_else(|| darken_color(bg, 0.70));
            let tab_bg = ui("tab.inactiveBackground").unwrap_or_else(|| chrome_darker.clone());
            let tab_active_bg = ui("tab.activeBackground").unwrap_or_else(|| chrome.clone());
            let titlebar_bg =
                ui("titleBar.activeBackground").unwrap_or_else(|| chrome_darker.clone());
            let line_number_fg = ui("editorLineNumber.foreground")
                .unwrap_or_else(|| if is_dark { "#6e7681" } else { "#999999" }.into());
            let line_number_active_fg =
                ui("editorLineNumber.activeForeground").unwrap_or_else(|| {
                    fg_color
                        .clone()
                        .unwrap_or_else(|| if is_dark { "#e0e0e0" } else { "#1e1e1e" }.into())
                });

            let fg = fg_color
                .as_deref()
                .unwrap_or(if is_dark { "#e0e0e0" } else { "#1e1e1e" });
            let sidebar_fg = ui("sideBar.foreground").unwrap_or_else(|| fg.to_string());
            let fg_dim = ui("descriptionForeground")
                .unwrap_or_else(|| if is_dark { "#aaaaaa" } else { "#555555" }.into());
            let separator = ui("sideBar.border")
                .unwrap_or_else(|| if is_dark { "#1a1a1a" } else { "#cccccc" }.into());
            let gutter_bg = ui("editorGutter.background").unwrap_or_else(|| editor_bg.clone());
            let button_bg = ui("button.background").unwrap_or_else(|| darken_color(&chrome, 0.85));
            let button_fg = ui("button.foreground").unwrap_or_else(|| fg.to_string());
            let button_hover_bg = darken_color(&button_bg, 0.85);
            let input_bg = ui("input.background").unwrap_or_else(|| darken_color(bg, 0.8));
            let input_border = ui("input.border").unwrap_or_else(|| separator.clone());

            let css_str = format!(
                r#"
                /* ── Global backgrounds ── */
                window,
                window.background {{
                    background-color: {chrome};
                    color: {fg};
                }}

                /* ── Header bar ── */
                headerbar {{
                    background: {titlebar_bg};
                    color: {fg};
                    min-height: 28px;
                    border-bottom: 1px solid {separator};
                    box-shadow: none;
                }}
                button.windowcontrol {{
                    min-width: 20px;
                    min-height: 20px;
                    padding: 2px;
                    margin: 0;
                }}
                button.windowcontrol image {{
                    -gtk-icon-size: 14px;
                }}

                /* ── Left pane: vertical icon nav bar + panels ── */
                box.left-nav {{
                    background: {chrome_darker};
                    padding: 4px 0;
                }}
                box.left-nav > button {{
                    background: transparent;
                    color: {sidebar_fg};
                    border: none;
                    box-shadow: none;
                    border-radius: 0;
                    min-height: 36px;
                    min-width: 36px;
                    padding: 6px;
                    margin: 0;
                }}
                box.left-nav > button:hover {{
                    background: {chrome};
                    color: {fg};
                }}
                box.left-nav > button:checked {{
                    background: {chrome};
                    color: {fg};
                    box-shadow: inset 2px 0 0 {fg};
                }}
                box.left-nav > button image {{
                    -gtk-icon-size: 18px;
                }}
                stack {{
                    background-color: {chrome};
                }}

                /* ── Tab bars in notebooks ── */
                notebook > header {{
                    background: {tab_bg};
                    border-bottom: 1px solid {separator};
                }}
                notebook > header tab {{
                    background: {tab_bg};
                    color: {fg_dim};
                    border: none;
                    box-shadow: none;
                    padding: 4px 12px;
                }}
                notebook > header tab:checked {{
                    background: {tab_active_bg};
                    color: {fg};
                    border-bottom: 2px solid {fg};
                }}

                /* ── Buttons ── */
                button {{
                    background: {button_bg};
                    color: {button_fg};
                    border: 1px solid {separator};
                    box-shadow: none;
                }}
                button:hover {{
                    background: {button_hover_bg};
                }}
                button.flat {{
                    background: transparent;
                    border: none;
                }}
                button.flat:hover {{
                    background: {chrome};
                }}

                /* ── Labels, entries ── */
                label {{
                    color: {fg};
                }}
                entry, searchentry {{
                    background: {input_bg};
                    color: {fg};
                    border: 1px solid {input_border};
                    box-shadow: none;
                }}
                textview.commit-input {{
                    background: {input_bg};
                    color: {fg};
                    border: 1px solid {input_border};
                    border-radius: 6px;
                }}
                textview.commit-input text {{
                    background: transparent;
                    color: {fg};
                }}
                searchentry text {{
                    color: {fg};
                }}

                /* ── Dim/secondary text ── */
                .dim-label {{
                    color: {fg_dim};
                }}

                /* ── Paned separators: hidden for flat look ── */
                paned > separator {{
                    min-width: 0;
                    min-height: 0;
                    background: transparent;
                    opacity: 0;
                }}

                /* ── Scrollbar blend ── */
                scrollbar {{
                    background-color: transparent;
                }}

                /* ── List views (file browser, search results) ── */
                listview {{
                    background-color: {chrome};
                    color: {sidebar_fg};
                }}
                listview > row {{
                    color: {sidebar_fg};
                }}

                /* ── Popover menus (right-click) ── */
                popover {{
                    background: {chrome_darker};
                    color: {fg};
                }}
                popover modelbutton {{
                    color: {fg};
                }}

                /* ── Editor line number gutter ── */
                textview.view border.left gutter {{
                    background-color: {gutter_bg};
                }}
                textview border.left {{
                    background-color: {gutter_bg};
                }}
                .line-numbers {{
                    color: {line_number_fg};
                    background-color: {gutter_bg};
                }}
                .current-line-number {{
                    color: {line_number_active_fg};
                }}
                "#
            );

            css_str
        }
        None => {
            // No custom background — respect system default and just fix separators
            if let Some(settings) = gtk4::Settings::default() {
                settings.set_gtk_application_prefer_dark_theme(false);
            }
            r#"
                paned > separator {
                    min-width: 0;
                    min-height: 0;
                    background: transparent;
                    opacity: 0;
                }
            "#
            .to_owned()
        }
    };

    // Status bar color: use the activity bar or chrome-darker background.
    let status_bar_bg = bg_color
        .as_ref()
        .map(|bg| {
            syntax_theme
                .as_ref()
                .and_then(|t| t.ui_color("statusBar.background"))
                .map(|s| s.to_string())
                .unwrap_or_else(|| darken_color(bg, 0.70))
        })
        .unwrap_or_default();
    let status_bar_fg = bg_color
        .as_ref()
        .map(|bg| {
            syntax_theme
                .as_ref()
                .and_then(|t| t.ui_color("statusBar.foreground"))
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    if perceived_brightness(bg) < 128 {
                        "#cccccc".to_string()
                    } else {
                        "#555555".to_string()
                    }
                })
        })
        .unwrap_or_default();

    let status_bar_css = if !status_bar_bg.is_empty() {
        format!(
            r#"
            .status-bar {{
                background: {status_bar_bg};
                padding: 2px 0;
                min-height: 20px;
            }}
            .status-bar-label {{
                color: {status_bar_fg};
                font-size: 11px;
            }}
            .status-bar-icon {{
                color: {status_bar_fg};
                -gtk-icon-size: 12px;
            }}
            .status-bar-blame {{
                color: {status_bar_fg};
                font-size: 11px;
            }}

            /* ── Branch switcher popover ── */
            popover.branch-popover > contents {{
                background: {status_bar_bg};
                color: {status_bar_fg};
                padding: 0;
            }}
            .branch-list {{
                background-color: {status_bar_bg};
            }}
            .branch-list > row {{
                background-color: transparent;
                color: {status_bar_fg};
                padding: 4px 0;
                border-bottom: 1px solid alpha({status_bar_fg}, 0.1);
            }}
            .branch-list > row:hover {{
                background-color: alpha({status_bar_fg}, 0.08);
            }}
            .branch-list > row:selected {{
                background-color: alpha({status_bar_fg}, 0.15);
            }}
            .branch-name {{
                color: {status_bar_fg};
                font-weight: bold;
                font-size: 13px;
            }}
            .branch-current {{
                color: #73c991;
            }}
            "#
        )
    } else {
        r#"
            .status-bar {
                padding: 2px 0;
                min-height: 20px;
            }
            .status-bar-label { font-size: 11px; }
            .status-bar-icon { -gtk-icon-size: 12px; }
            .status-bar-blame { font-size: 11px; }
            .branch-name { font-weight: bold; font-size: 13px; }
            .branch-current { color: #73c991; }
        "#
        .to_string()
    };

    // Git status colors — try VS Code theme colors first, fall back to defaults.
    let git_modified_color = syntax_theme
        .as_ref()
        .and_then(|t| t.ui_color("gitDecoration.modifiedResourceForeground"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "#e2c08d".to_string());
    let git_added_color = syntax_theme
        .as_ref()
        .and_then(|t| t.ui_color("gitDecoration.untrackedResourceForeground"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "#73c991".to_string());
    let git_deleted_color = syntax_theme
        .as_ref()
        .and_then(|t| t.ui_color("gitDecoration.deletedResourceForeground"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "#c74e39".to_string());
    let git_renamed_color = "#73b8e2";

    // Append git status badge colors and compact list row spacing.
    let git_css = format!(
        r#"
        .git-status-m {{ color: {git_modified_color}; }}
        .git-status-a {{ color: {git_added_color}; }}
        .git-status-d {{ color: {git_deleted_color}; }}
        .git-status-r {{ color: {git_renamed_color}; }}
        .git-status-c {{ color: #e06c75; }}
        .file-git-modified {{ color: {git_modified_color}; }}
        .file-git-added {{ color: {git_added_color}; }}
        .file-git-deleted {{ color: {git_deleted_color}; }}
        .file-git-renamed {{ color: {git_renamed_color}; }}
        listview.compact-list > row {{ padding: 0; min-height: 0; }}
        listview.compact-list > row > cell {{ padding: 0; margin: 0; min-height: 0; }}
        listview.compact-list button.circular {{ min-height: 16px; min-width: 16px; padding: 0; margin: 0; }}
        textview.commit-input {{ border-radius: 6px; }}
        textview.commit-input text {{ background: transparent; }}
    "#
    );
    let full_css = format!("{css}\n{status_bar_css}\n{git_css}");

    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&full_css);

    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Parse a hex color and return its perceived brightness (0–255).
///
/// Uses the standard luminance formula: `0.299*R + 0.587*G + 0.114*B`.
fn perceived_brightness(hex: &str) -> u8 {
    let (r, g, b) = parse_hex(hex);
    (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) as u8
}

/// Darken a hex color string by the given factor (0.0 = black, 1.0 = unchanged).
///
/// Accepts `#RRGGBB` or `#RGB` format. Returns `#RRGGBB`.
fn darken_color(hex: &str, factor: f64) -> String {
    let (r, g, b) = parse_hex(hex);
    let r = (r as f64 * factor) as u8;
    let g = (g as f64 * factor) as u8;
    let b = (b as f64 * factor) as u8;
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Parse `#RRGGBB` or `#RGB` into (r, g, b).
fn parse_hex(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        (
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(0),
        )
    } else if hex.len() == 3 {
        (
            u8::from_str_radix(&hex[0..1], 16).unwrap_or(0) * 17,
            u8::from_str_radix(&hex[1..2], 16).unwrap_or(0) * 17,
            u8::from_str_radix(&hex[2..3], 16).unwrap_or(0) * 17,
        )
    } else {
        (0x2e, 0x2e, 0x2e) // fallback dark grey
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_darken_color_black_unchanged() {
        assert_eq!(darken_color("#000000", 0.85), "#000000");
    }

    #[test]
    fn test_darken_color_white_darkened() {
        assert_eq!(darken_color("#ffffff", 0.5), "#7f7f7f");
    }

    #[test]
    fn test_darken_color_factor_one_unchanged() {
        assert_eq!(darken_color("#abcdef", 1.0), "#abcdef");
    }

    #[test]
    fn test_darken_color_short_hex() {
        assert_eq!(darken_color("#fff", 0.5), "#7f7f7f");
    }

    #[test]
    fn test_darken_color_no_hash() {
        assert_eq!(darken_color("ffffff", 0.5), "#7f7f7f");
    }

    #[test]
    fn test_perceived_brightness_white() {
        assert_eq!(perceived_brightness("#ffffff"), 255);
    }

    #[test]
    fn test_perceived_brightness_black() {
        assert_eq!(perceived_brightness("#000000"), 0);
    }

    #[test]
    fn test_perceived_brightness_dark_theme() {
        // Typical dark theme bg like #2e3436 should be well below 128
        assert!(perceived_brightness("#2e3436") < 128);
    }

    #[test]
    fn test_perceived_brightness_light_theme() {
        // Typical light theme bg like #f5f5f5 should be above 128
        assert!(perceived_brightness("#f5f5f5") > 128);
    }

    #[test]
    fn test_parse_hex_six_digit() {
        assert_eq!(parse_hex("#abcdef"), (0xab, 0xcd, 0xef));
    }

    #[test]
    fn test_parse_hex_three_digit() {
        assert_eq!(parse_hex("#f00"), (0xff, 0x00, 0x00));
    }
}
