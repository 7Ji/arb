# Rootless

ARB operates fully without root permission, the main component, arb, runs in the ancestor/parent namespaces, without root permission, with a non-root UID and a non-root GID.

When arb needs to emulate root permission, it would spawn a child process (broker) that would unshare its user, mount, network namespaces from the root, which would then optionally spawn a non-arb child process within the target root.

