# 1: pkgbuild
LIBRARY="${LIBRARY:-/usr/share/makepkg}"
source "${LIBRARY}/"util.sh
source "${LIBRARY}/"source.sh
source_makepkg_config
source $1
get_all_sources_for_arch 'all_sources'
i=0
for source in "${all_sources[@]}"; do
  echo '[source]'
  echo "name:$(get_filename "${source}")"
  protocol=$(get_protocol "${source}")
  echo "protocol:${protocol}"
  url=$(get_url "${source}")
  case "${protocol}" in
    bzr)
      if [[ $url != bzr+ssh* ]]; then
        url=${url#bzr+}
      fi
      url=${url%%#*}
      ;;
    fossil)
      url=${url#fossil+}
      url=${url%%#*}
      url=${url%%\?*}
      ;;
    git)
      url=${url#git+}
      url=${url%%#*}
      url=${url%%\?*}
      ;;
    hg)
      url=${url#hg+}
      url=${url%%#*}
      ;;
    svn)
      if [[ $url != svn+ssh* ]]; then
        url=${url#svn+}
      fi
      url=${url%%#*}
      ;;
  esac
  echo "url:${url}"
  for _integ in {ck,md5,sha{1,224,256,384,512},b2}; do
    declare -n checksums="${_integ}sums"
    checksum="${checksums[$i]}"
    case "${checksum}" in
    ''|'SKIP') :;;
    *)
      echo "${_integ}sum:${checksum}"
      ;;
    esac
  done
  let i++
done