use std::io::Read;

pub(super) fn md5sum(file: &mut std::fs::File) -> Option<[u8; 16]> {
    let mut context = md5::Context::new();
    let mut buffer = vec![0; super::BUFFER_SIZE];
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                eprintln!("Failed to read file: {}", e);
                return None
            },
        };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        context.consume(chunk);
    }
    Some(context.compute().0)
}