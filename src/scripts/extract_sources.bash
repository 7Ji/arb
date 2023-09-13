# 1: pkgbuild name to enter
LIBRARY="${LIBRARY:-/usr/share/makepkg}"
source "${LIBRARY}/"util.sh
source "${LIBRARY}/"source.sh
source_makepkg_config
source "$1"/PKGBUILD
get_all_sources_for_arch 'all_sources'
(
  i=0
  for source in "${all_sources[@]}"; do
    protocol=$(get_protocol "${source}")
    url=$(get_url "${source}")
    name=$(get_filename "${source}")
    case "${protocol}" in
      bzr|fossil|hg|svn|local) :;;
      git)
        url=${url#git+}
        url=${url%%#*}
        url=${url%%\?*}
        ln -sf ../../sources/git/$(printf '%s' "${url}" | xxhsum -H3 | cut -d ' ' -f 4) "$1/$name"
        ;;
      *)
        for _integ in {ck,md5,sha{1,224,256,384,512},b2}; do
          declare -n checksums="${_integ}sums"
          checksum="${checksums[$i]}"
          case "${checksum}" in
          ''|'SKIP') :;;
          *)
            ln -sf ../../sources/file-"${_integ}/${checksum}" "$1/$name"
            ;;
          esac
        done
        ;;
    esac
    i=$(( i + 1 ))
  done
)
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