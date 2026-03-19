use crossterm::{
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    ExecutableCommand,
};
use std::io::{self, Write};

#[allow(dead_code)]
const LOGO: &str = r#"
oooooooooo.    .oooooo.   ooooooooo.     .oooooo.
`888'   `Y8b  d8P'  `Y8b  `888   `Y88.  d8P'  `Y8b
 888     888 888      888  888   .d88' 888
 888oooo888' 888      888  888ooo88P'  888
 888    `88b 888      888  888`88b.    888     ooooo
 888    .88P `88b    d88'  888  `88b.  `88.    .88'
o888bood8P'   `Y8bood8P'  o888o  o888o  `Y8bood8P'
"#;

#[allow(dead_code)]
pub fn print_logo() -> anyhow::Result<()> {
    let mut stdout = io::stdout();
    stdout.execute(SetForegroundColor(Color::Rgb {
        r: 0,
        g: 185,
        b: 174,
    }))?;
    stdout.execute(SetAttribute(Attribute::Bold))?;
    stdout.execute(Print(LOGO))?;
    stdout.execute(ResetColor)?;
    stdout.execute(SetAttribute(Attribute::Reset))?;
    stdout.flush()?;
    Ok(())
}
