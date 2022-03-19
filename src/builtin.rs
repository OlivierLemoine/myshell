use std::{cmp::Ordering, fmt, fs};

use prettytable::{Cell, Row};
use rlua::UserData;

#[derive(Clone)]
pub struct TableRes {
    header: Vec<String>,
    entries: Vec<Vec<String>>,
}
impl fmt::Display for TableRes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut table = prettytable::Table::new();
        table.set_format(*prettytable::format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR);
        table.set_titles(Row::new(self.header.iter().map(|v| Cell::new(v)).collect()));
        for entry in &self.entries {
            table.add_row(Row::new(entry.iter().map(|v| Cell::new(v)).collect()));
        }

        write!(f, "{table}")
    }
}
impl UserData for TableRes {}

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
        header: vec!["Type".to_string(), "Name".to_string()],
        entries,
    }
}
