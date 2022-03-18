use std::fs;



pub fn ls(dir: &str) -> Vec<String> {
    fs::read_dir(if dir.is_empty() { "." } else { dir })
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_string())
        .collect()
}
