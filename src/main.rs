mod builtin;

use std::{
    fs,
    io::{stdout, Read, Write},
    process::{self, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use builtin::TableRes;
use crossterm::{
    cursor::{position, EnableBlinking, MoveTo, MoveToNextLine},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::Print,
    terminal::{disable_raw_mode, enable_raw_mode, size, Clear, ClearType, ScrollUp},
};
use is_executable::IsExecutable;
use rlua::{Lua, ToLua, Variadic};

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

    let query =
        tree_sitter::Query::new(tree_sitter_lua::language(), "(assignment_statement)").unwrap();

    let lua = Lua::new();

    let should_tty = Arc::new(Mutex::new(false));

    for path in env!("PATH").split(':') {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    if let Some(name) = entry.file_name().to_str() {
                        let path = entry.path();
                        if path.is_executable() {
                            let should_tty = Arc::clone(&should_tty);
                            let lua_res: BoxedRes<()> = lua.context(move |lua_ctx| {
                                let path = path;
                                let globals = lua_ctx.globals();

                                let call_fn = lua_ctx.create_function(
                                    move |lua_ctx, args: Variadic<String>| {
                                        let path = path.clone();

                                        let mut cmd = process::Command::new(&path);
                                        cmd.args(args.iter().collect::<Vec<_>>());

                                        let should_tty = *should_tty.lock().unwrap();

                                        if should_tty {
                                            disable_raw_mode().unwrap();
                                            cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
                                        } else {
                                            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
                                        }

                                        let output =
                                            cmd.spawn().unwrap().wait_with_output().unwrap();

                                        Ok(if !should_tty {
                                            TableRes {
                                                header: vec![
                                                    "path".to_string(),
                                                    "code".to_string(),
                                                    "stdout".to_string(),
                                                    "stderr".to_string(),
                                                ],
                                                entries: vec![vec![
                                                    path.to_str().unwrap().to_string(),
                                                    output.status.code().unwrap_or(-1).to_string(),
                                                    std::str::from_utf8(&output.stdout)
                                                        .unwrap()
                                                        .trim()
                                                        .to_string(),
                                                    std::str::from_utf8(&output.stderr)
                                                        .unwrap()
                                                        .trim()
                                                        .to_string(),
                                                ]],
                                            }
                                            .to_lua(lua_ctx)
                                            .unwrap()
                                        } else {
                                            enable_raw_mode().unwrap();
                                            rlua::Value::Nil
                                        })
                                    },
                                )?;
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

    let init_code_path = home::home_dir().map(|mut p| {
        p.push(".config/myshell/init.lua");
        p
    });
    if let Some(init_code) = init_code_path.and_then(|p| fs::read_to_string(p).ok()) {
        lua.context::<_, BoxedRes<()>>(|lua_ctx| {
            lua_ctx.load(&init_code).exec()?;
            Ok(())
        })?;
    }

    let mut must_draw = true;
    let ps1 = String::from("❯ ");
    let mut cmd = String::new();

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

                        let code = cmd.trim();

                        let mut parser = tree_sitter::Parser::new();
                        parser.set_language(tree_sitter_lua::language())?;
                        let tree = parser
                            .parse(&code, None)
                            .ok_or_else(|| format!("Can't parse lua code : {code}"))?;
                        let node = tree.root_node();

                        let print_tty = if node.child_count() <= 1 {
                            let mut query_cursor = tree_sitter::QueryCursor::new();
                            if query_cursor
                                .matches(&query, tree.root_node(), |_| "")
                                .count()
                                == 0
                            {
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        *should_tty.lock().unwrap() = print_tty;

                        match lua.context::<_, BoxedRes<String>>(|lua_ctx| {
                            Ok(match lua_ctx.load(&cmd).eval::<rlua::Value>()? {
                                rlua::Value::UserData(data) => match data.borrow::<TableRes>() {
                                    Ok(table) => {
                                        disable_raw_mode()?;
                                        table.as_display_table().print_tty(true);
                                        enable_raw_mode()?;
                                        String::new()
                                    }
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
