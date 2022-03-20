mod builtin;

use std::{
    fs,
    io::{stdout, Read, Write},
    process::{self, Stdio},
    sync::{Arc, Mutex},
    thread,
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
use is_executable::IsExecutable;
use rlua::{Lua, Variadic};

fn print(s: &str) -> BoxedRes<()> {
    let mut stdout = stdout();

    let term_height = size()?.1;
    let cursor_height = position()?.1;
    let available_space = term_height - cursor_height;

    for (i, l) in s.split('\n').enumerate() {
        if i >= available_space as usize {
            queue!(stdout, ScrollUp(1))?;
        }

        if i > 0 {
            queue!(stdout, MoveToNextLine(1))?;
        }
        queue!(stdout, Print(l))?;
    }

    queue!(stdout, EnableBlinking)?;

    stdout.flush()?;

    Ok(())
}

type BoxedRes<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> BoxedRes<()> {
    enable_raw_mode()?;

    let lua = Lua::new();

    for path in env!("PATH").split(':') {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    if let Some(name) = entry.file_name().to_str() {
                        let path = entry.path();
                        if path.is_executable() {
                            let lua_res: BoxedRes<()> = lua.context(move |lua_ctx| {
                                let path = path;
                                let globals = lua_ctx.globals();

                                let call_fn =
                                    lua_ctx.create_function(move |_, args: Variadic<String>| {
                                        let path = path.clone();
                                        let mut child = process::Command::new(path)
                                            .args(args.iter().collect::<Vec<_>>())
                                            .stdout(Stdio::piped())
                                            .stderr(Stdio::piped())
                                            .spawn()
                                            .unwrap();

                                        let mut child_stdout = child.stdout.take().unwrap();
                                        let mut child_stderr = child.stderr.take().unwrap();

                                        loop {
                                            match child.try_wait() {
                                                Ok(None) => {
                                                    let mut buff = String::new();
                                                    child_stdout.read_to_string(&mut buff).unwrap();
                                                    print(&buff).unwrap();

                                                    let mut buff = String::new();
                                                    child_stderr.read_to_string(&mut buff).unwrap();
                                                    print(&buff).unwrap();

                                                    thread::sleep(Duration::from_millis(100));
                                                }
                                                Ok(Some(_)) => {
                                                    break;
                                                }
                                                Err(_) => {
                                                    break;
                                                }
                                            }
                                        }

                                        let mut buff = String::new();
                                        child_stdout.read_to_string(&mut buff).unwrap();
                                        print(&buff).unwrap();

                                        Ok(())
                                    })?;
                                globals.set(name, call_fn)?;

                                Ok(())
                            });
                            lua_res?;
                        }
                    }
                }
            }
        }
    }

    lua.context::<_, BoxedRes<()>>(|lua_ctx| {
        let globals = lua_ctx.globals();

        let ls = lua_ctx.create_function(|_, path: Variadic<String>| {
            let path = path.first().map(|v| v as &str).unwrap_or_else(|| ".");
            Ok(builtin::ls(path))
        })?;
        globals.set("ls", ls)?;

        let cd = lua_ctx.create_function(|_, path: Variadic<String>| {
            let path = path.first().map(|v| v as &str).unwrap_or_else(|| "");
            Ok(builtin::cd(path))
        })?;
        globals.set("cd", cd)?;

        let print = lua_ctx.create_function(|_, s: String| {
            print(&s).unwrap();
            Ok(())
        })?;
        globals.set("__internal_print", print)?;

        lua_ctx
            .load(
                "function print(...)
                    for i = 1, select('#', ...) do
                        __internal_print(tostring(select(i, ...)))
                    end
                end",
            )
            .exec()
            .unwrap();

        Ok(())
    })?;

    let mut must_draw = true;
    let ps1 = String::from("❯ ");
    let mut cmd = String::new();
    let all = String::from("Welcome !");

    execute!(stdout(), Print(&all), MoveToNextLine(1))?;

    let cursor_position = Arc::new(Mutex::new(position()?));
    let cursor_pos = Arc::clone(&cursor_position);

    let print_cmd = move |s: &str| {
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
            print_cmd(&cmd);
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
                        print("\n")?;

                        match lua.context::<_, BoxedRes<String>>(|lua_ctx| {
                            Ok(match lua_ctx.load(&cmd).eval::<rlua::Value>()? {
                                rlua::Value::UserData(data) => match data.borrow::<TableRes>() {
                                    Ok(table) => table.to_string(),
                                    Err(_) => String::new(),
                                },
                                rlua::Value::String(s) => s.to_str()?.to_string(),
                                rlua::Value::Error(err) => TableRes {
                                    header: vec!["Error".to_string()],
                                    entries: vec![vec![err.to_string()]],
                                }
                                .to_string(),
                                _ => String::new(),
                            })
                        }) {
                            Ok(res) => {
                                print(&res)?;
                                print("\n")?;

                                *cursor_position.lock().unwrap() = position()?;
                                cmd = String::new();
                                must_draw = true;
                            }
                            Err(e) => {
                                print(&e.to_string())?;
                                print("\n")?;
                                must_draw = true;
                            }
                        }
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
