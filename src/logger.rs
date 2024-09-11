use std::{fs::OpenOptions, io::Write};

pub fn log_to_file(path: &str, data: &str) {
    let file_result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);

    if let Ok(mut file) = file_result {
        let to_write = format!("[{}]: {}\n", chrono::offset::Local::now(), data);
        let _ = file.write_all(to_write.as_bytes());
    }
}

