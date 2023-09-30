# 1: pkgbuild name to enter
LIBRARY="${LIBRARY:-/usr/share/makepkg}"
source "${LIBRARY}/"util.sh
source "${LIBRARY}/"source.sh
source_makepkg_config
source "$1"/PKGBUILD
SRCDEST="$1"
HOLDVER=1
download_sources
srcdir="${SRCDEST}"/src
mkdir "${srcdir}"
cd "${srcdir}"
extract_sources
if [[ "$(type -t prepare)" == 'function' ]]; then
    prepare
fi