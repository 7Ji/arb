LIBRARY="${LIBRARY:-/usr/share/makepkg}"
source "${LIBRARY}/"util.sh
source "${LIBRARY}/"source.sh
source_makepkg_config
dump_array_with_optional_arch() { #1: var name, 2: report name
  declare -n array="$1"
  declare -n array_arch="$1_${CARCH}"
  for item in "${array[@]}" "${array_arch[@]}"; do
    echo "$2:${item}"
  done
}
while read -r line; do
  source ./"${line}"
  echo "[PKGBUILD]"
  echo "base:${pkgbase:-${pkgname}}"
  for item in "${pkgname[@]}"; do
    echo "name:${item}"
  done
  dump_array_with_optional_arch depends dep
  dump_array_with_optional_arch makedepends makedep
  dump_array_with_optional_arch provides provide
  dump_array_with_optional_arch source source
  for integ in {ck,md5,sha{1,224,256,384,512},b2}; do
    dump_array_with_optional_arch "${integ}"sums "${integ}"
  done
  echo -n "pkgver_func:"
  if [[ $(type -t pkgver) == 'function' ]]; then echo y; else echo n; fi
  unset -f pkgver
  unset depends provides
  eval $(declare -f package | sed --quiet 's/ \+\(depends=.\+\);/\1/p; s/ \+\(provides=.\+\);/\1/p')
  dump_array_with_optional_arch depends dep
  dump_array_with_optional_arch provides provide
  for item in "${pkgname[@]}"; do
    unset depends provides
    eval $(declare -f package_"${item}" | sed --quiet 's/ \+\(depends=.\+\);/\1/p; s/ \+\(provides=.\+\);/\1/p')
    dump_array_with_optional_arch depends dep_"${item}"
    dump_array_with_optional_arch provides provide_"${item}"
  done
  unset pkgbase pkgname {depends,makedepends,provides,source}{,_"${CARCH}"}
  for _integ in {ck,md5,sha{1,224,256,384,512},b2}; do
    unset "${_integ}sums" "${_integ}sums_${CARCH}"
  done
done