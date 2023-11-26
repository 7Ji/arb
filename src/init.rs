use std::env::set_current_dir;

use libc::{uid_t, gid_t};
use nix::{unistd::{pivot_root, setgroups, setgid, Gid, setuid, Uid}, mount::{umount2, MntFlags}, sys::prctl::{self, set_child_subreaper, set_no_new_privs}};

const BUILDER_UID: Uid = Uid::from_raw(1000);
const BUILDER_GID: Gid = Gid::from_raw(1000);

/// The basic fake-init 
/// 
/// 
/// 
/// 
pub(crate) fn prepare() {
    set_no_new_privs();
    set_child_subreaper(true);
    set_current_dir("/tmp/newroot");
    pivot_root(".", ".");
    umount2(".", MntFlags::MNT_DETACH);
}

/// Drop to the hardcoded builder uid and gid
pub(crate) fn drop() {
    setgroups(&[]);
    setgid(BUILDER_GID);
    setuid(BUILDER_UID);
}

/// Shorthand to call prepare() then drop()
pub(crate) fn prepare_and_drop() {
    prepare();
    drop();
}


pub(crate) fn finish() {

}