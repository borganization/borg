use crossterm::{
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    ExecutableCommand,
};
use std::io::{self, Write};

const LOGO: &str = r#"
       ╭━━━━━━━╮
      ╱ ●    ● ╲
     │    ‿‿    │
     │          │
    ╱╲╱╲╱╲╱╲╱╲╱╲
    ┃  tamago   ┃
    ╰━━━━━━━━━━━╯
"#;

pub fn print_logo() -> anyhow::Result<()> {
    let mut stdout = io::stdout();
    stdout.execute(SetForegroundColor(Color::Magenta))?;
    stdout.execute(SetAttribute(Attribute::Bold))?;
    stdout.execute(Print(LOGO))?;
    stdout.execute(ResetColor)?;
    stdout.execute(SetAttribute(Attribute::Reset))?;
    stdout.flush()?;
    Ok(())
}
