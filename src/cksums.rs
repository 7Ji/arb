use std::fmt::Display;

use crc;

const CKSUM: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_CKSUM);

fn cksum(input: &[u8]) {
    let mut digest = CKSUM.digest();
    digest.update(input);
    let mut len_oct = Vec::<u8>::new();
    let mut len = input.len();
    if len > 0 {
        while len > 0 {
            len_oct.push((len & 0xFF).try_into().unwrap());
            len >>= 8;
        }
    } else {
        len_oct.push(0);
    }
    digest.update(&len_oct);
    println!("No length: {}, has length: {}", CKSUM.checksum(input), digest.finalize());
}

pub(crate) fn optional_equal<C:PartialEq>(a: &Option<C>, b: &Option<C>) -> bool {
    if let Some(a) = a {
        if let Some(b) = b {
            if a == b {
                return true
            }
        }
    }
    false
}

pub(crate) fn optional_update<C>(target: &mut Option<C>, source: &Option<C>) 
where C: PartialEq + Clone {
    if let Some(target) = target {
        if let Some(source) = source {
            if target == source {
                return;
            } else {
                panic!("Source target mismatch");
            }
        } else {
            return;
        }
    } else if let Some(source) = source {
        *target = Some(source.clone())
    }
}

pub(crate) fn print(cksum: &[u8]) -> String {
    String::from_iter(cksum.iter().map(|byte| format!("{:02x}", byte)))
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
