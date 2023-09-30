use std::io::Read;
use hex::FromHex;

#[derive(PartialEq, Clone)]
pub(crate) struct Md5sum ([u8; 16]);

impl super::Sum for Md5sum {
    fn sum(file: &mut std::fs::File) -> Option<Self> {
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
        Some(Self(context.compute().0))
    }

    fn from_hex(hex: &[u8]) -> Option<Self> where Self: Sized {
        Some(Self(FromHex::from_hex(hex).ok()?))
    }
}

impl std::fmt::Display for Md5sum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0.iter() {
            if let Err(e) = f.write_fmt(format_args!("{:02x}", byte)) {
                return Err(e);
            }
        }
        Ok(())
    }
}