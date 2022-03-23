mod builtin;

use std::{
    fs,
    io::{stdout, Write},
    process::{self, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use builtin::TableRes;
use crossterm::{
    cursor::{position, EnableBlinking, MoveTo, MoveToNextLine, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::Print,
    terminal::{disable_raw_mode, enable_raw_mode, size, Clear, ClearType, ScrollUp},
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

struct Command {
    cmd: Vec<String>,
    cursor_initial: (u16, u16),
    cursor: (usize, usize),
    redraw: bool,
    ps1: String,
}
impl Default for Command {
    fn default() -> Self {
        Command {
            cmd: vec![String::new()],
            cursor_initial: position().unwrap(),
            cursor: (0, 0),
            redraw: true,
            ps1: "$ ".to_string(),
        }
    }
}
impl Command {
    fn draw(&mut self, lua: &Lua) -> BoxedRes<()> {
        if self.redraw {
            self.ps1 = lua.context(|lua_ctx| {
                let globals = lua_ctx.globals();
                let config = globals.get::<_, rlua::Table>("config")?;
                let ps1 = config.get::<_, rlua::Function>("ps1")?;
                ps1.call::<_, String>(())
            })?;

            let mut stdout = stdout();
            queue!(
                stdout,
                MoveTo(self.cursor_initial.0, self.cursor_initial.1),
                Clear(ClearType::FromCursorDown),
                Print(&self.ps1),
            )?;

            let mut after_ps1 = position()?;

            let term_height = size()?.1;
            let cursor_height = self.cursor_initial.1;
            let available_space = term_height - cursor_height;

            for (i, l) in self.cmd.iter().enumerate() {
                if i >= available_space as usize {
                    queue!(stdout, ScrollUp(1))?;
                    self.cursor_initial.1 -= 1;
                    after_ps1.1 -= 1;
                }

                if i > 0 {
                    queue!(stdout, MoveToNextLine(1))?;
                }
                queue!(stdout, Print(l)).unwrap();
            }

            if self.cursor.1 == 0 {
                queue!(
                    stdout,
                    MoveTo(after_ps1.0 + self.cursor.0 as u16, after_ps1.1),
                )?;
            } else {
                queue!(
                    stdout,
                    MoveTo(self.cursor.0 as u16, after_ps1.1 + self.cursor.1 as u16),
                )?;
            }

            queue!(stdout, Show)?;

            stdout.flush()?;

            self.redraw = false;
        }

        Ok(())
    }

    fn reset_cursor_initial(&mut self) {
        self.cursor_initial = position().unwrap();
        self.redraw = true;
    }

    fn code(&self) -> String {
        self.cmd.join("\n")
    }

    fn add_char(&mut self, c: char) {
        match c {
            '\r' => {}
            '\n' => {
                let (line, new_line) = self.cmd[self.cursor.1].split_at(self.cursor.0);
                let new_line = new_line.to_string();
                let line = line.to_string();
                self.cmd[self.cursor.1] = line;
                self.cursor = (0, self.cursor.1 + 1);
                self.cmd.insert(self.cursor.1, new_line);
            }
            c => {
                self.cmd[self.cursor.1].insert(self.cursor.0, c);
                self.cursor.0 += 1;
            }
        }

        self.redraw = true;
    }

    fn remove_char(&mut self) {
        match self.cursor.0 {
            0 if self.cursor.1 > 0 => {
                let line = self.cmd.remove(self.cursor.1);
                self.cursor.1 -= 1;
                self.cursor.0 = self.cmd[self.cursor.1].len();
                self.cmd[self.cursor.1].extend(line.chars());

                self.redraw = true;
            }
            0 => {
                // Nothing
            }
            x => {
                self.cmd[self.cursor.1].remove(x - 1);
                self.cursor.0 -= 1;

                self.redraw = true;
            }
        }
    }

    fn left(&mut self) -> bool {
        match self.cursor.0 {
            0 => {
                // Nothing
                false
            }
            _ => {
                self.cursor.0 -= 1;
                self.redraw = true;
                true
            }
        }
    }

    fn right(&mut self, wrapping: bool) -> bool {
        match self.cursor.0 {
            x if x == self.cmd[self.cursor.1].len() => {
                if wrapping {
                    if self.down() {
                        self.cursor.0 = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    // Nothing
                    false
                }
            }
            _ => {
                self.cursor.0 += 1;
                self.redraw = true;
                true
            }
        }
    }

    fn up(&mut self) -> bool {
        match self.cursor.0 {
            0 => {
                // Nothing
                false
            }
            _ => {
                self.cursor.1 -= 1;
                self.cursor.0 = self.cursor.0.min(self.cmd[self.cursor.1].len());
                self.redraw = true;
                true
            }
        }
    }

    fn down(&mut self) -> bool {
        match self.cursor.1 {
            x if x < self.cmd.len() - 1 => {
                self.cursor.1 += 1;
                self.cursor.0 = self.cursor.0.min(self.cmd[self.cursor.1].len());
                self.redraw = true;
                true
            }
            _ => {
                //Nothing
                false
            }
        }
    }
}

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

                                        let should_tty_lock = *should_tty.lock().unwrap();

                                        if should_tty_lock {
                                            disable_raw_mode().unwrap();
                                            cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
                                        } else {
                                            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
                                        }

                                        let output =
                                            cmd.spawn().unwrap().wait_with_output().unwrap();

                                        let table = lua_ctx.create_table()?;
                                        table.set("code", output.status.code())?;
                                        table.set("path", path.to_str().unwrap().to_string())?;
                                        match should_tty_lock {
                                            true => {
                                                enable_raw_mode().unwrap();
                                                *should_tty.lock().unwrap() = false;
                                            }
                                            false => {
                                                table.set(
                                                    "stdout",
                                                    std::str::from_utf8(&output.stdout)
                                                        .unwrap()
                                                        .trim()
                                                        .to_string(),
                                                )?;
                                                table.set(
                                                    "stderr",
                                                    std::str::from_utf8(&output.stderr)
                                                        .unwrap()
                                                        .trim()
                                                        .to_string(),
                                                )?;
                                            }
                                        }

                                        Ok(table)
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
                r#"function print(...)
                    for i = 1, select('#', ...) do
                        __internal_print(tostring(select(i, ...)))
                    end
                end

                config = { ps1 = function() return "$ " end }
                "#,
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

    //let ps1 = String::from("❯ ");
    let mut cmd = Command::default();

    loop {
        cmd.draw(&lua)?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(KeyEvent { code, modifiers }) => match (code, modifiers) {
                    (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                        break;
                    }
                    (KeyCode::Backspace, m) if m.is_empty() => cmd.remove_char(),
                    (KeyCode::Char(' '), KeyModifiers::CONTROL) => {
                        print("\n")?;

                        let code = cmd.code();

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
                            Ok(match lua_ctx.load(&code).eval::<rlua::Value>()? {
                                rlua::Value::UserData(data) => match data.borrow::<TableRes>() {
                                    Ok(table) => {
                                        disable_raw_mode()?;
                                        table.as_display_table().print_tty(true);
                                        enable_raw_mode()?;
                                        String::new()
                                    }
                                    Err(_) => String::new(),
                                },
                                rlua::Value::Table(table) => {
                                    let mut t = prettytable::Table::new();
                                    t.set_format(*prettytable::format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR);
                                    table
                                        .pairs::<rlua::Value, rlua::Value>()
                                        .filter_map(|pair| pair.ok())
                                        .filter_map(|(key, value)| match (key, value) {
                                            (rlua::Value::Integer(_), rlua::Value::String(s)) => s
                                                .to_str()
                                                .map(|v| {
                                                    prettytable::Row::new(vec![
                                                        prettytable::Cell::new(v),
                                                    ])
                                                })
                                                .ok(),
                                            (rlua::Value::String(k), rlua::Value::String(s)) => k
                                                .to_str()
                                                .and_then(|k| s.to_str().map(|v| (k, v)))
                                                .map(|(k, v)| {
                                                    prettytable::Row::new(vec![
                                                        prettytable::Cell::new(k),
                                                        prettytable::Cell::new(v),
                                                    ])
                                                })
                                                .ok(),
                                            (a, b) => unimplemented!("{a:?} {b:?}"),
                                        })
                                        .for_each(|r| {
                                            t.add_row(r);
                                        });

                                    t.to_string()
                                }
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
                                cmd = Command::default();
                            }
                            Err(e) => {
                                print(&e.to_string())?;
                                print("\n")?;
                                cmd.reset_cursor_initial();
                            }
                        }

                        *should_tty.lock().unwrap() = false;
                    }
                    (KeyCode::Left, m) if m.is_empty() => {
                        cmd.left();
                    }
                    (KeyCode::Right, m) if m.is_empty() => {
                        cmd.right(false);
                    }
                    (KeyCode::Up, m) if m.is_empty() => {
                        cmd.up();
                    }
                    (KeyCode::Down, m) if m.is_empty() => {
                        cmd.down();
                    }
                    (KeyCode::Delete, m) if m.is_empty() => {
                        if cmd.right(true) {
                            cmd.remove_char()
                        }
                    }
                    (KeyCode::Enter, m) if m.is_empty() => cmd.add_char('\n'),
                    (KeyCode::Char(c), _) => cmd.add_char(if modifiers == KeyModifiers::SHIFT {
                        c.to_uppercase().next().unwrap()
                    } else {
                        c
                    }),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}
