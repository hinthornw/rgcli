use crossterm::style::Stylize;

fn user_label() -> String {
    format!("{}", "You: ".blue().bold())
}

fn assistant_label() -> String {
    format!("{}", "Assistant: ".green())
}

fn system_style(text: &str) -> String {
    format!("{}", text.dark_grey().italic())
}

fn error_style(text: &str) -> String {
    format!("{}", text.red().bold())
}

fn prompt_style(text: &str) -> String {
    format!("{}", text.cyan().bold())
}

fn logo_accent(text: &str) -> String {
    format!("{}", text.dark_yellow())
}

fn logo_body(text: &str) -> String {
    format!("{}", text.dark_cyan())
}

fn logo_title(text: &str) -> String {
    format!("{}", text.cyan().bold())
}

pub fn print_logo(version: &str, endpoint: &str, config_path: &str) {
    let title = format!("{} {}", logo_title("lsc"), system_style(version));
    let info1 = system_style(endpoint);
    let info2 = system_style(config_path);

    let lines = [
        format!("   {}", logo_accent("▄█▀▀█▄")),
        format!(
            "  {}{}{}    {}",
            logo_accent("▄██"),
            logo_body("▄░▄"),
            logo_accent("█"),
            title
        ),
        format!("  {}    {}", logo_body("███████"), info1),
        format!("  {}     {}", logo_body("▀█░░░█"), info2),
        format!("   {}", logo_body("█▀ █▀")),
    ];

    for line in lines {
        println!("{line}");
    }
}

pub fn print_error(msg: &str) -> String {
    error_style(&format!("Error: {}", msg))
}

pub fn user_prompt() -> String {
    prompt_style("> ")
}

pub fn user_prefix() -> String {
    user_label()
}

pub fn assistant_prefix() -> String {
    assistant_label()
}

pub fn system_text(msg: &str) -> String {
    system_style(msg)
}
