/* Tar handling, this is meanly used in the following sceneraios:
  - to cache PKGBUILD with their extracted sources (with assumption: extraction takes time would be slow), this operates in non-root mode
  - to restore cached PKGBUILD with their extracted sources, this operates in non-root mode
  - to cache roots with installed packages, this operates in root mode
  - to restore cached root with installed packages
*/