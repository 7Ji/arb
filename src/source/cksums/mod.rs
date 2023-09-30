mod ck;
mod crypto;
mod integ;
mod md5;

const BUFFER_SIZE: usize = 0x400000; // 4M

trait Sum {
    fn sum(file: &mut std::fs::File) -> Option<Self> where Self: Sized;
}