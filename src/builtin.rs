use std::{cmp::Ordering, env, fmt, fs, path::PathBuf, str::FromStr, vec};

use prettytable::{Cell, Row};
use rlua::{Lua, MetaMethod, ToLua, UserData};

#[derive(Debug, Default, Clone)]
pub struct TableRes {
    pub header: Vec<String>,
    pub entries: Vec<Vec<String>>,
}
impl TableRes {
    pub fn as_display_table(&self) -> prettytable::Table {
        let mut table = prettytable::Table::new();
        table.set_format(*prettytable::format::consts::FORMAT_CLEAN);
        table.set_titles(Row::new(
            self.header
                .iter()
                .map(|v| Cell::new(v).style_spec("biuc"))
                .collect(),
        ));
        for entry in &self.entries {
            table.add_row(Row::new(entry.iter().map(|v| Cell::new(v)).collect()));
        }

        table
    }
}
impl fmt::Display for TableRes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = self.as_display_table();

        write!(f, "{table}")
    }
}
impl UserData for TableRes {
    fn add_methods<'lua, T: rlua::UserDataMethods<'lua, Self>>(methods: &mut T) {
        methods.add_meta_function(
            MetaMethod::Index,
            |lua_ctx, (table, idx): (TableRes, rlua::Value)| match idx {
                rlua::Value::Integer(idx) => table
                    .entries
                    .get(idx as usize - 1)
                    .map(|v| {
                        lua_ctx
                            .create_table_from(
                                table
                                    .header
                                    .iter()
                                    .zip(v.iter())
                                    .map(|(k, v)| (k.clone(), v.clone())),
                            )
                            .and_then(|v| v.to_lua(lua_ctx))
                    })
                    .unwrap_or_else(|| lua_ctx.create_table().and_then(|v| v.to_lua(lua_ctx))),
                rlua::Value::String(col) => {
                    let col = col.to_str()?;

                    table
                        .header
                        .iter()
                        .position(|v| v == col)
                        .map(|idx| {
                            table
                                .entries
                                .iter()
                                .map(move |v| v[idx].clone())
                                .enumerate()
                        })
                        .map(|it| {
                            lua_ctx
                                .create_table_from(it)
                                .and_then(|v| v.to_lua(lua_ctx))
                        })
                        .unwrap_or_else(|| lua_ctx.create_table().and_then(|v| v.to_lua(lua_ctx)))
                }
                _ => Ok(rlua::Value::Nil),
            },
        );
        methods.add_meta_function(MetaMethod::ToString, |_, table: TableRes| {
            Ok(table.to_string())
        });
        methods.add_meta_function(MetaMethod::Len, |_, table: TableRes| {
            Ok(table.entries.len())
        });
    }
}

pub fn ls(dir: &str) -> TableRes {
    let mut entries = fs::read_dir(if dir.is_empty() { "." } else { dir })
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.metadata().ok().map(|metadata| (entry, metadata)))
        .map(|(entry, metadata)| {
            vec![
                match (metadata.is_dir(), metadata.is_file(), metadata.is_symlink()) {
                    (true, false, false) => "dir".to_string(),
                    (false, true, false) => "file".to_string(),
                    (false, false, true) => "sym".to_string(),
                    _ => unreachable!(),
                },
                entry.file_name().to_str().unwrap().to_string(),
            ]
        })
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| match (a[0].as_str(), b[0].as_str()) {
        (x, y) if x == y => a[1].cmp(&b[1]),
        ("sym", _) => Ordering::Greater,
        ("dir", "file") => Ordering::Greater,
        _ => Ordering::Less,
    });

    TableRes {
        header: vec!["type".to_string(), "name".to_string()],
        entries,
    }
}

pub fn cd(dir: &str) {
    if let Some(path) = if dir.is_empty() {
        home::home_dir().or_else(|| env::current_dir().ok())
    } else {
        PathBuf::from_str(dir).ok()
    } {
        env::set_current_dir(&path).unwrap();
    }
}

//pub fn pipe(lua: &Lua, )
