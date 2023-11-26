arb uses user namespaces to create rootless containers for all interaction with the PKGBUILDs, including but not limited to: 
- parsing the PKGBUILD to convert them to Rust-friendly internal format
- extracting the PKGBUILD and preparing the source
- bootstrapping the chroot for pkgs

As fork.2 is not well incorporated into the Rust eco for async signal safety, and clone.2 is even more out of the box, we have to do a lot of dances around the namespaces. Basically, we:
- converted arb to a busybox-like multi-call program
- has an broker applet that would be spawned by the main arb executable, which would create mounting points before passing down to init, it starts
- has a dummy init that would 