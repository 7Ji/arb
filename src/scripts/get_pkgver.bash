srcdir=$(readlink -f build/"$1"/src)
cd "${srcdir}"
source ../PKGBUILD
pkgver