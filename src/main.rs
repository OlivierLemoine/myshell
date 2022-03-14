use std::{io::stdout, time::Duration};

use crossterm::{
    cursor::{position, MoveTo, MoveToNextLine},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::Print,
    terminal::{enable_raw_mode, Clear, ClearType},
};

type BoxedRes<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> BoxedRes<()> {
    enable_raw_mode()?;

    let mut must_draw = true;
    let mut ps1 = String::from("$");
    let mut cmd = String::new();
    let mut all = String::from("Welcome !");

    execute!(
        stdout(),
        Print(&all),
        Print("\r\n"),
        MoveToNextLine(10),
        Print(&ps1),
    )?;

    let mut cursor_position = position()?;

    let print = |s: &str| {
        execute!(
            stdout(),
            MoveTo(cursor_position.0, cursor_position.1),
            Clear(ClearType::FromCursorDown),
            Print(s),
        )
        .unwrap();
    };

    loop {
        if must_draw {
            print(&format!("{ps1}{cmd}"));
            must_draw = false;
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(KeyEvent { code, modifiers }) => match (code, modifiers) {
                    (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                        break;
                    }
                    (KeyCode::Backspace, m) if m.is_empty() => {
                        if cmd.len() > 0 {
                            cmd.remove(cmd.len() - 1);
                        }

                        must_draw = true;
                    }
                    (KeyCode::Enter, m) if m.is_empty() => {
                        cmd.push('\n');

                        must_draw = true;
                    }
                    (KeyCode::Char(c), _) => {
                        cmd.push(if modifiers == KeyModifiers::SHIFT {
                            c.to_uppercase().next().unwrap()
                        } else {
                            c
                        });

                        must_draw = true;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}
