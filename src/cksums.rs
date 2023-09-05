use crc;

const CKSUM: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_CKSUM);

fn cksum(input: &[u8]) {
    let mut digest = CKSUM.digest();
    digest.update(input);
    let mut len_oct = Vec::<u8>::new();
    let mut len = input.len();
    while len > 0 {
        len_oct.push((len & 0xFF).try_into().unwrap());
        len >>= 8;
    }
    digest.update(&len_oct);
    println!("No length: {}, has length: {}", CKSUM.checksum(input), digest.finalize());
}

// const CKSUM: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_CKSUM);
// fn cksum() {
//     const INPUT: &[u8] = b"123456789";
//     let mut d = CKSUM.digest();
//     d.update(&
//         INPUT);
//     let mut i = f.len();
//     while i > 0 {
//         let rem: u8 = (i & 0xFF).try_into().expect("Failed to convert to u8");
//         d.update(&[rem]);
//         i >>= 8;
//     }
//     // d.update(bytes)
//     println!("{:x}", d.finalize());
//     return;
// }
