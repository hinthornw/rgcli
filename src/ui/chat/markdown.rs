use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

pub(super) fn render_markdown_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_language: Option<String> = None;
    let mut code_lines: Vec<String> = Vec::new();

    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = ts
        .themes
        .get("base16-ocean.dark")
        .unwrap_or_else(|| ts.themes.values().next().unwrap());

    for raw_line in text.lines() {
        if raw_line.starts_with("```") {
            if in_code_block {
                render_code_block(&ps, theme, &code_language, &code_lines, &mut lines);
                code_lines.clear();
                code_language = None;
                in_code_block = false;
            } else {
                in_code_block = true;
                code_language = raw_line.strip_prefix("```").map(|s| s.trim().to_string());
            }
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::new().fg(Color::DarkGray),
            )));
            continue;
        }

        if in_code_block {
            code_lines.push(raw_line.to_string());
            continue;
        }

        // Headers
        if let Some(h) = raw_line.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                h.to_string(),
                Style::new().add_modifier(Modifier::BOLD).fg(Color::Cyan),
            )));
        } else if let Some(h) = raw_line.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                h.to_string(),
                Style::new().add_modifier(Modifier::BOLD).fg(Color::Cyan),
            )));
        } else if let Some(h) = raw_line.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                h.to_string(),
                Style::new()
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    .fg(Color::Cyan),
            )));
        } else if let Some(item) = raw_line
            .strip_prefix("- ")
            .or_else(|| raw_line.strip_prefix("* "))
        {
            lines.push(Line::from(format!("  • {item}")));
        } else {
            let spans = parse_inline_markdown(raw_line);
            lines.push(Line::from(spans));
        }
    }

    // Handle unclosed code block
    if in_code_block && !code_lines.is_empty() {
        render_code_block(&ps, theme, &code_language, &code_lines, &mut lines);
    }

    lines
}

fn render_code_block(
    ps: &SyntaxSet,
    theme: &syntect::highlighting::Theme,
    language: &Option<String>,
    code_lines: &[String],
    output: &mut Vec<Line<'static>>,
) {
    if let Some(lang) = language {
        let syntax = ps
            .find_syntax_by_token(lang)
            .unwrap_or_else(|| ps.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, theme);

        for code_line in code_lines {
            let highlighted = highlighter
                .highlight_line(code_line, ps)
                .unwrap_or_default();
            let spans: Vec<Span<'static>> = highlighted
                .into_iter()
                .map(|(style, text)| {
                    Span::styled(format!("  {}", text), syntect_to_ratatui_style(style))
                })
                .collect();
            output.push(Line::from(spans));
        }
    } else {
        for code_line in code_lines {
            output.push(Line::from(Span::styled(
                format!("  {code_line}"),
                Style::new().fg(Color::Green),
            )));
        }
    }
}

fn syntect_to_ratatui_style(style: SyntectStyle) -> Style {
    let fg = style.foreground;
    Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b))
}

pub(super) fn parse_inline_markdown(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find("**") {
            if pos > 0 {
                spans.push(Span::raw(remaining[..pos].to_string()));
            }
            let after = &remaining[pos + 2..];
            if let Some(end) = after.find("**") {
                spans.push(Span::styled(
                    after[..end].to_string(),
                    Style::new().add_modifier(Modifier::BOLD),
                ));
                remaining = &after[end + 2..];
            } else {
                spans.push(Span::raw(remaining[pos..].to_string()));
                break;
            }
        } else if let Some(pos) = remaining.find('`') {
            if pos > 0 {
                spans.push(Span::raw(remaining[..pos].to_string()));
            }
            let after = &remaining[pos + 1..];
            if let Some(end) = after.find('`') {
                spans.push(Span::styled(
                    after[..end].to_string(),
                    Style::new().fg(Color::Yellow),
                ));
                remaining = &after[end + 1..];
            } else {
                spans.push(Span::raw(remaining[pos..].to_string()));
                break;
            }
        } else {
            spans.push(Span::raw(remaining.to_string()));
            break;
        }
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}
