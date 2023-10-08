// A single-package GET request 'https://aur.archlinux.org/rpc/v5/info/ampart'
// {
//     "resultcount": 1,
//     "results": [
//         {
//             "Depends": [
//                 "glibc"
//                 "zlib"
//             ],
//             "Description": "A partition tool to modify Amlogic's proprietary eMMC partition format and FDT",
//             "FirstSubmitted": 1677652518,
//             "ID": 1219346,
//             "Keywords": [],
//             "LastModified": 1677652518,
//             "License": ["GPL3"],
//             "Maintainer": "7Ji",
//             "MakeDepends": ["gcc"],
//             "Name": "ampart",
//             "NumVotes": 0,
//             "OutOfDate": null, 
//             "PackageBase": "ampart",
//             "PackageBaseID": 190927,
//             "Popularity": 0,
//             "Submitter": "7Ji",
//             "URL": "https://github.com/7Ji/ampart",
//             "URLPath": "/cgit/aur.git/snapshot/ampart.tar.gz", 
//             "Version": "1.3-1"
//         }
//     ],
//     "type": "multiinfo",
//     "version":5
// }
// Basically the only part we need is LastModified, and by comparing that against our FETCH_HEAD we could therefore determine whether it needs update

struct _AurPkg {
    name: String,
    last_modified: u64
}

struct _AurPkgs {
    pkgs: Vec<_AurPkg>
}

impl _AurPkgs {

}