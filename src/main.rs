mod builtin;

use std::{
    io::{stdout, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use builtin::TableRes;
use crossterm::{
    cursor::{position, EnableBlinking, MoveTo, MoveToNextLine},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::Print,
    terminal::{enable_raw_mode, size, Clear, ClearType, ScrollUp},
};
use rlua::{Lua, Variadic};

type BoxedRes<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> BoxedRes<()> {
    enable_raw_mode()?;

    let lua = Lua::new();

    let lua_res: BoxedRes<()> = lua.context(|lua_ctx| {
        let globals = lua_ctx.globals();

        let ls = lua_ctx.create_function(|_, path: Variadic<String>| {
            let path = path.first().map(|v| v as &str).unwrap_or_else(|| ".");
            Ok(builtin::ls(path))
        })?;
        globals.set("ls", ls)?;

        Ok(())
    });
    lua_res?;

    let mut must_draw = true;
    let ps1 = String::from("❯ ");
    let mut cmd = String::new();
    let all = String::from("Welcome !");

    execute!(stdout(), Print(&all), MoveToNextLine(1))?;

    let cursor_position = Arc::new(Mutex::new(position()?));
    let cursor_pos = Arc::clone(&cursor_position);

    let print = move |s: &str| {
        let mut cursor_position = cursor_pos.lock().unwrap();
        let mut stdout = stdout();
        queue!(
            stdout,
            MoveTo(cursor_position.0, cursor_position.1),
            Clear(ClearType::FromCursorDown),
            Print(&ps1),
        )
        .unwrap();

        let term_height = size().unwrap().1;
        let cursor_height = cursor_position.1;
        let available_space = term_height - cursor_height;

        for (i, l) in s.split('\n').enumerate() {
            if i >= available_space as usize {
                queue!(stdout, ScrollUp(1)).unwrap();
                cursor_position.1 -= 1;
            }

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
                    (KeyCode::Char(' '), KeyModifiers::CONTROL) => {
                        let res: BoxedRes<String> = lua.context(|lua_ctx| {
                            let res = lua_ctx.load(&cmd).eval::<TableRes>()?;
                            Ok(format!("{res}"))
                        });

                        let res = res?;

                        print(&format!("{cmd}\n{res}\n"));
                        *cursor_position.lock().unwrap() = position()?;
                        cmd = String::new();
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
