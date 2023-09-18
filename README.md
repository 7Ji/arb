# Arch Repository Builder

A multi-threaded builder to build packages and create a sane folder structure for an Arch repo, written initially for https://github.com/7Ji/archrepo

## Build
Run the following command inside this folder
```
cargo build --release
```
Optionally, strip the binary so it would take less space
```
strip target/release/arch_repo_builder -o somewhere/convenient/to/run/it/from
```
The output binary would be `target/release/arch_repo_builder`

## Usage
```
Usage: arch_repo_builder [OPTIONS] [PKGBUILDS]

Arguments:
  [PKGBUILDS]  Optional PKGBUILDs.yaml file [default: PKGBUILDs.yaml]

Options:
  -p, --proxy <PROXY>  HTTP proxy to retry for git updating and http(s) netfiles if attempt without proxy failed
  -P, --holdpkg        Hold versions of PKGBUILDs, do not update them
  -G, --holdgit        Hold versions of git sources, do not update them
  -I, --skipint        Skip integrity check for netfile sources if they're found
  -B, --nobuild        Do not actually build the packages
  -C, --noclean        Do not clean unused sources
  -h, --help           Print help
  -V, --version        Print version
```
The `PKGBUILDs.yaml` would contain simple lines of `name: url`, e.g.:
```
ampart: https://aur.archlinux.org/ampart.git/
chormium-mpp: https://aur.archlinux.org/chromium-mpp.git
yaopenvfd: https://aur.archlinux.org/yaopenvfd.git
```

## Internal
The builder does the following to save a great chunk of build time and resource:
 1. All PKGBUILDs are maintained locally as bare git repos under `sources/PKGBUILDs`, and these repos are updated multi-threadedly, 1 thread per domain (I don't want to put too much load on AUR server)
 2. All git sources are cached locally under `sources/git`, and the update process could take 4 thread per domain.
 3. All network file sources, as long as they have integrity checksums, are cached locally under `sources/file-[integ name]`, and the download process could take 4 thread per domain. And if a file source has multiple checksums, it would only be downloaded once, all remaining cache files are just hard-linked from the first one.
 4. The git and netfile cacher run simultaneously
 5. Build folders `build/[package]` are only populated (also multi-threaded) if either:
    1. The corresponding package has a `pkgver()` function which could only be run after complete source extraction
    2. The corresponding pkgdir `pkg/[pkgid]` is missing, in which `[pkgid]` is generated with `[name]-[commit](-[pkgver])`
 6. Build folder is populated via lightweight checkout (no `.git`) from the local PKGBUILDs bare repos, and symlinks of cached sources. Only vcs sources not with git protocol and netfile sources that do not have integrity checks need to be downloaded for each build.
 7. Packages are stored under `pkg/[pkgid]`. Two folders, `pkg/updated` and `pkg/latest` are created with symlinks, `updated` containing links to packages built during the current run, and `latest` containing links to all latest packages.
    1. `updated` is useful when partial update is wanted
    2. `latest` is useful when full update is wanted