use std::ffi::OsStr;

use nix::unistd::pivot_root;

/// Init: A dummy init implementation that just passes its args to downstream
/// But wait, it does more:
/// - It also pivot_root while it still has the perm, path is hardcoded
/// - It then drop perms by setuid
/// 
/// Those are also done by other init-able applets
// fn main() {
//     pivot_root()
// }

fn main<I, S>(args: I) 
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>
{
    crate::init::prepare_and_drop();
    



    crate::init::finish()
}