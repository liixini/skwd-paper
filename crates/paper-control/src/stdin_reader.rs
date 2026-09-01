pub fn spawn_stdin_line_reader<F>(tag: &'static str, mut on_line: F)
where
    F: FnMut(&str) + Send + 'static,
{
    std::thread::spawn(move || {
        use std::io::BufRead;

        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut line = String::new();
        loop {
            line.clear();
            match handle.read_line(&mut line) {
                Ok(0) => {
                    eprintln!("{tag}: stdin closed, exiting");
                    unsafe { libc::_exit(0) };
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        on_line(trimmed);
                    }
                }
                Err(error) => {
                    eprintln!("{tag}: stdin read error: {error}");
                    unsafe { libc::_exit(0) };
                }
            }
        }
    });
}
