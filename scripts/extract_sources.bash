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
get_filepath() {
    local file="$(get_filename "$1")"
    file="../$file"
    local proto="$(get_protocol "$1")"
    case $proto in
        bzr|git|hg|svn)
            if [[ ! -d "$file" ]]; then
                return 1
            fi
        ;;
        *)
            if [[ ! -f "$file" ]]; then
                return 1
            fi
        ;;
    esac
    printf "%s\n" "$file"
}

extract_sources
if [[ "$(type -t prepare)" == 'function' ]]; then
    prepare
fi