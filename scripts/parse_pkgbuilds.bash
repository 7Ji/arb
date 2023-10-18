LIBRARY="${LIBRARY:-/usr/share/makepkg}"
source "${LIBRARY}/"util.sh
source "${LIBRARY}/"source.sh
source_makepkg_config
while read -r line; do
  source "${line}"
  echo "[PKGBUILD]"
  echo "base:${pkgbase:-${pkgname}}"
  for item in "${pkgname[@]}"; do
    echo "name:${item}"
  done
  for item in "${depends[@]}"; do
    echo "dep:${item}"
  done
  for item in "${makedepends[@]}"; do
    echo "makedep:${item}"
  done
  for item in "${provides[@]}"; do
    echo "provide:${item}"
  done
  declare -n source_arch=source_"${CARCH}"
  for item in "${source[@]}" "${source_arch[@]}"; do
    echo "source:${item}"
  done
  for integ in {ck,md5,sha{1,224,256,384,512},b2}; do
    declare -n checksums="${integ}sums"
    declare -n checksums_arch="${integ}sums_${CARCH}"
    for item in "${checksums[@]}" "${checksums_arch[@]}"; do
      echo "${integ}:${item}"
    done
  done
  echo -n "pkgver_func:"
  if [[ $(type -t pkgver) == 'function' ]]; then echo y; else echo n; fi
  unset -f pkgver
  unset pkgbase pkgname depends makedepends provides source source_"${CARCH}" pkgver
  for _integ in {ck,md5,sha{1,224,256,384,512},b2}; do
    unset "${_integ}sums" "${_integ}sums_${CARCH}"
  done
done