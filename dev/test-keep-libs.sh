#!/usr/bin/env bash
# Перевірка pacman-хука keep-libs без root і без змін у системі.
#
# Генерує keep-libs з рушія, підключає його (source — тоді main не виконується), спрямовує
# його теки в тимчасову й підміняє кеш ldconfig відфільтрованим, щоб бібліотека «зникла» із
# системи. Бере встановлені пакети *-legacy-580xx і справжню базу pacman (лише читає): запускати
# на машині з OBS-Legacy.
#
#   bash dev/test-keep-libs.sh
set -uo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/keep-libs-test.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
fails=0

check() {   # опис, умова (команда)
  local what=$1
  shift
  if "$@"; then
    printf 'ok   %s\n' "$what"
  else
    printf 'FAIL %s\n' "$what"
    fails=$((fails + 1))
  fi
}

ours=$(pacman -Qq 2>/dev/null | grep -E -- '-legacy-580xx$' || true)
if [[ -z "$ours" ]]; then
  echo "skip: no *-legacy-580xx packages are installed"
  exit 0
fi

# Скрипт хука такий, як він іде в пакет OBS. Повідомлення — англійською (їх перевіряє grep).
export LC_ALL='' LC_MESSAGES='' LANG=C.UTF-8
OBS_LEGACY_LANG=en bash -c 'source <(sed "\$d" "$1"); write_pkg_keeplibs "$2"' _ \
  "$repo/scripts/build-obs-nvenc-legacy-artix.sh" "$work/keep-libs" || { echo "could not generate keep-libs"; exit 1; }
bash -n "$work/keep-libs" || exit 1

# shellcheck source=/dev/null
source "$work/keep-libs"
# shellcheck disable=SC2034 # їх читає підключений keep-libs
kept=$work/kept staged=$work/staged list=$work/state/kept-libs.tsv
mkdir -p "$kept"
hide=''
ldmap() {
  if [[ -n "$hide" ]]; then LC_ALL=C command ldconfig -p | grep -vE "$hide"; else LC_ALL=C command ldconfig -p; fi
}
ldconfig() { :; }

# Бібліотека, потрібна пакету OBS-Legacy, з іншого пакета (не glibc і не gcc-libs: їх
# позначити «зниклими» — нереалістично).
load_needs
lib='' owner=''
for s in $(printf '%s\n' "${!NEED[@]}" | sort); do
  path=$(LC_ALL=C command ldconfig -p | awk -v so="$s" '/x86-64/ && $1 == so { print $NF; exit }')
  [[ -n "$path" ]] || continue
  o=$(LC_ALL=C pacman -Qqo -- "$path" 2>/dev/null || true)
  if [[ -n "$o" && "$o" != *-legacy-580xx && "$o" != glibc && "$o" != gcc-libs ]]; then lib=$s owner=$o; break; fi
done
[[ -n "$lib" ]] || { echo "skip: no library of another package found"; exit 0; }
echo "library: $lib (package $owner, needed by ${NEED[$lib]})"
re=${lib//./\\.}

out=$(printf '%s\n' "$owner" | pre)
check "pre copies a library of a package in the transaction" test -f "$staged/$lib"
check "pre prints nothing" test -z "$out"

hide="^[[:space:]]*$re "
out=$(post)
check "post keeps the copy when the system no longer has it" test -f "$kept/$lib"
check "post writes who needs it" grep -q "^$re	" "$list"
check "post says so at the end of the pacman output" grep -q "kept copies of libraries" <<<"$out"
check "nothing staged is left" test ! -e "$staged"

post >/dev/null
check "a second post keeps it while it is still missing" test -f "$kept/$lib"

hide=''
out=$(post)
check "post removes the copy once the system has the library again" test ! -e "$kept/$lib"
check "post removes the list" test ! -e "$list"
check "post says the copies are gone" grep -q "no longer needed" <<<"$out"

out=$(post)
check "post with nothing kept is silent" test -z "$out"

printf 'no-such-package-xyz\n' | pre
check "pre for unrelated packages copies nothing" test ! -e "$staged"

if ((fails)); then
  echo "$fails check(s) failed"
  exit 1
fi
echo "all checks passed"
