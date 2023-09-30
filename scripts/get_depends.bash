# 1: pkgbuild
. "$1"
for dep in "${depends[@]}" "${makedepends[@]}"; do
    echo "${dep}"
done