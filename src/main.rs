use std::{
    io::{stdout, Write},
    time::Duration,
};

use crossterm::{
    cursor::{position, EnableBlinking, MoveTo, MoveToNextLine},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
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

    execute!(stdout(), Print(&all), MoveToNextLine(1))?;

    let mut cursor_position = position()?;

    let print = |s: &str| {
        let mut stdout = stdout();
        queue!(
            stdout,
            MoveTo(cursor_position.0, cursor_position.1),
            Clear(ClearType::FromCursorDown),
            Print(&ps1),
        )
        .unwrap();

        for (i, l) in s.split('\n').enumerate() {
            if i > 0 {
                queue!(stdout, MoveToNextLine(1)).unwrap();
            }
            queue!(stdout, Print(l)).unwrap();
        }

        queue!(stdout, EnableBlinking).unwrap();

        stdout.flush().unwrap();
    };

    loop {
        if must_draw {
            print(&cmd);
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
