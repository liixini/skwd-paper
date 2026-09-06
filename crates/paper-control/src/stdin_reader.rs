pub fn spawn_stdin_line_reader<F>(tag: &'static str, mut on_line: F)
where
    F: FnMut(&str) + Send + 'static,
{
    std::thread::spawn(move || {
        use std::io::{BufRead, Write};

        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut line = String::new();
        loop {
            line.clear();
            match handle.read_line(&mut line) {
                Ok(0) => {
                    let _ = writeln!(std::io::stderr(), "{tag}: stdin closed, exiting");
                    unsafe { libc::_exit(0) };
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        on_line(trimmed);
                    }
                }
                Err(error) => {
                    let _ = writeln!(std::io::stderr(), "{tag}: stdin read error: {error}");
                    unsafe { libc::_exit(0) };
                }
            }
        }
    });
}
