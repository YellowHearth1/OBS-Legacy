#!/usr/bin/env bash
# Тест автоматичного оновлення разом із системою (packaging/auto-update) без root: файл
# підключається (source), шляхи, pacman, obs-legacy, setsid і користувач підставляються.
# Запуск: bash dev/test-auto-update.sh — нічого в системі не змінює.
set -u
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
T=$(mktemp -d)
trap 'rm -rf -- "$T"' EXIT
fails=0
pass() { printf '  ✓ %s\n' "$1"; }
bad() { printf '  ✗ %s\n' "$1"; fails=$((fails + 1)); }
check() { if eval "$2"; then pass "$1"; else bad "$1"; fi; }

LC_ALL=C.UTF-8
# shellcheck source=packaging/auto-update
source "$here/packaging/auto-update"

me=$(id -un) my_uid=$(id -u) my_gid=$(id -g)
mkdir -p "$T/home/store" "$T/sys" "$T/out"
# shellcheck disable=SC2034 # змінні добудовувача, підставлені для тесту
info=$T/sys/BUILDINFO.txt kept_list=$T/sys/kept-libs.tsv state=$T/state log_dir=$T/log
log=$T/log/auto-update.log work=$T/work run_lock=$T/run.lock
# shellcheck disable=SC2034 # те саме
db_wait=4
mkdir -p "$state" "$log_dir"
program=$T/obs-legacy
self=$T/auto-update-self

# Підставний obs-legacy: --repair з кодом REPAIR_RC, --install-files — рядки з $T/files.
cat > "$program" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$T/out/program.calls"
case "\$1" in
  --repair) echo "Звіт: $T/home/report.txt"; exit "\${REPAIR_RC:-0}" ;;
  --install-files) cat "$T/files" 2>/dev/null ;;
esac
EOF
chmod 755 "$program"

# Лише «я» — звичайний користувач; root та інші — ні.
user_entry() {
  if [[ "$1" == "$me" || "$1" == "$my_uid" ]]; then printf '%s:%s:%s:%s\n' "$me" "$my_uid" "$my_gid" "$T/home"; else return 1; fi
}
as_user() { "$@"; }
db_lock_path() { printf '%s\n' "$T/db.lck"; }
setsid() { printf '%s\n' "$*" >> "$T/out/setsid.calls"; }
pacman() {
  local f
  # Власники файлів набору (kept_for_set): BUILDINFO.txt — OBS, ffmpeg.txt — ffmpeg. Запит, не дія.
  if [[ "$1" == -Qqo ]]; then
    for f in "${@:2}"; do
      case "${f##*/}" in
        BUILDINFO.txt) echo obs-studio-legacy-580xx ;;
        ffmpeg.txt) echo ffmpeg-legacy-580xx ;;
      esac
    done
    return 0
  fi
  # Встановлені пакети (take_file): усі, крім перелічених в ABSENT. Запит, не дія.
  if [[ "$1" == -Q ]]; then [[ " ${ABSENT:-} " != *" ${3:-} "* ]]; return; fi
  printf '%s|%s\n' "${OBS_LEGACY_AUTO_UPDATE:-}" "$*" >> "$T/out/pacman.calls"
  return "${PACMAN_RC:-0}"
}
getent() { :; }
# Адміністратор (wheel) — «я», доки ADMIN не 0.
is_admin() { [[ "$1" == "$me" && "${ADMIN:-1}" == 1 ]]; }
id() { if [[ "${1:-}" == -u ]]; then echo 0; else command id "$@"; fi; }

mkpkg() {   # файл, назва пакета
  local d
  d=$(mktemp -d)
  printf 'pkgname = %s\npkgver = 1-1\n' "$2" > "$d/.PKGINFO"
  bsdtar -czf "$1" -C "$d" .PKGINFO
  rm -rf -- "$d"
}
good=$T/home/store/obs-studio-legacy-580xx-33.0.3-14-x86_64.pkg.tar.gz
ff=$T/home/store/ffmpeg-legacy-580xx-2:9.1-1.3-x86_64.pkg.tar.gz
mkpkg "$good" obs-studio-legacy-580xx
mkpkg "$ff" ffmpeg-legacy-580xx
# Плагін, якого зараз немає в системі (власник замінив його своїм пакетом).
gone=$T/home/store/obs-gradient-source-legacy-580xx-0.6.4-14-x86_64.pkg.tar.gz
mkpkg "$gone" obs-gradient-source-legacy-580xx
mkpkg "$T/outside-legacy-580xx-1-1-x86_64.pkg.tar.gz" outside-legacy-580xx
ln -s "$T/outside-legacy-580xx-1-1-x86_64.pkg.tar.gz" "$T/home/store/link-legacy-580xx-1-1-x86_64.pkg.tar.gz"
mkpkg "$T/home/store/notes.txt" obs-studio-legacy-580xx
mkpkg "$T/home/store/evil-legacy-580xx-1-1-x86_64.pkg.tar.gz" coreutils
cat > "$T/files" <<EOF
install $good
install $ff
install $gone
install $T/outside-legacy-580xx-1-1-x86_64.pkg.tar.gz
install $T/home/store/link-legacy-580xx-1-1-x86_64.pkg.tar.gz
install $T/home/store/notes.txt
install $T/home/store/evil-legacy-580xx-1-1-x86_64.pkg.tar.gz
install store/relative-legacy-580xx-1-1-x86_64.pkg.tar.gz
remove obs-waveform-legacy-580xx
EOF
reset_out() { rm -rf -- "$T/out" "$log"; mkdir -p "$T/out"; }

echo "hook"
( unset SUDO_UID PKEXEC_UID DOAS_USER; hook ) > "$T/out/hook.txt"
check "без пакетів OBS-Legacy нічого не запускає" '[[ ! -e $T/out/setsid.calls ]]'
echo 'obs_version=33.0.3' > "$info"
touch "$T/sys/ffmpeg.txt"
( OBS_LEGACY_AUTO_UPDATE=1 SUDO_UID=$my_uid hook ) > "$T/out/hook.txt"
check "власний pacman -U не запускає себе знову" '[[ ! -e $T/out/setsid.calls ]]'
printf 'libjxl.so.0.12\tffmpeg-legacy-580xx\n' > "$kept_list"
( unset PKEXEC_UID DOAS_USER; SUDO_UID=$my_uid hook ) > "$T/out/hook.txt"
check "після sudo pacman запускає run від імені цього користувача" "grep -qx -- '-f $self run $me' '$T/out/setsid.calls'"
check "користувача запам'ятовано" "[[ \$(cat '$state/auto-update.user') == '$me' ]]"
check "pacman показує, що буде далі" "grep -q 'OBS-Legacy' '$T/out/hook.txt'"
reset_out
( unset PKEXEC_UID DOAS_USER; SUDO_UID=0 hook ) > "$T/out/hook.txt"
check "sudo від root → запам'ятований користувач" "grep -qx -- '-f $self run $me' '$T/out/setsid.calls'"
reset_out
( unset PKEXEC_UID DOAS_USER; SUDO_UID=0 ADMIN=0 hook ) > "$T/out/hook.txt"
check "запам'ятований користувач без прав адміністратора — нічого не запускає" "[[ ! -e '$T/out/setsid.calls' ]]"
reset_out
printf 'libolt.so.1\tolt-probe-legacy-580xx\n' > "$kept_list"
( unset PKEXEC_UID DOAS_USER; SUDO_UID=$my_uid hook ) > "$T/out/hook.txt"
check "копії лише для пакета не з набору — запускає, але не каже «перезбираю»" \
  "grep -qx -- '-f $self run $me' '$T/out/setsid.calls' && ! grep -q 'OBS-Legacy' '$T/out/hook.txt'"
printf 'libjxl.so.0.12\tffmpeg-legacy-580xx\n' > "$kept_list"
rm -f "$state/auto-update.user"; reset_out
( unset PKEXEC_UID DOAS_USER; SUDO_UID=0 DOAS_USER='-s' hook ) > "$T/out/hook.txt"
check "без відомого користувача нічого не запускає, лише порада" "[[ ! -e '$T/out/setsid.calls' ]] && grep -q 'OBS-Legacy' '$T/out/hook.txt'"

echo "run"
reset_out
touch "$T/db.lck"
( sleep 2; rm -f "$T/db.lck" ) &
ABSENT=obs-gradient-source-legacy-580xx run "$me" > "$T/out/run.txt" 2>&1
wait
check "чекає, доки pacman звільнить базу, і лагодить" "grep -qx -- '--repair' '$T/out/program.calls'"
check "ставить лише справжні пакети з теки користувача, одним pacman -U" \
  "[[ \$(wc -l < '$T/out/pacman.calls') == 1 ]] && grep -q '^1|-U --noconfirm -- .*/obs-studio-legacy-580xx-33.0.3-14-x86_64.pkg.tar.gz .*/ffmpeg-legacy-580xx-2:9.1-1.3-x86_64.pkg.tar.gz\$' '$T/out/pacman.calls'"
check "ставить копії з теки root, а не файли користувача" "! grep -q '$T/home' '$T/out/pacman.calls'"
check "відкидає 5 чужих рядків (поза текою, посилання, назва, .PKGINFO, відносний)" "[[ \$(grep -c 'is not an OBS-Legacy package' '$T/out/run.txt') == 5 ]]"
check "пакет, якого зараз немає в системі, не ставить знову" "grep -q 'obs-gradient-source-legacy-580xx.*is not installed now' '$T/out/run.txt' && ! grep -q 'gradient-source' '$T/out/pacman.calls'"
check "remove не виконує" "grep -q 'Leaving obs-waveform-legacy-580xx installed' '$T/out/run.txt' && ! grep -q -- '-R' '$T/out/pacman.calls'"
check "сповіщає про успіх" "grep -q -- '--auto-update-notice ok obs-studio-legacy-580xx' '$T/out/program.calls'"
check "у журналі — звіт лагодження" "grep -q '    Звіт: ' '$T/out/run.txt'"
check "робоча тека прибрана" "[[ ! -e '$work' ]]"

reset_out
cp "$T/files" "$T/files.all"; : > "$T/files"
REPAIR_RC=1 run "$me" > "$T/out/run.txt" 2>&1
check "невдале лагодження запам'ятовано й сповіщено" "[[ -s '$state/auto-update.failed' ]] && grep -q -- '--auto-update-notice failed' '$T/out/program.calls'"
reset_out
REPAIR_RC=1 run "$me" > "$T/out/run.txt" 2>&1
check "те саме вдруге не лагодить, лише шукає готові пакети" "! grep -qx -- '--repair' '$T/out/program.calls' && grep -qx -- '--install-files' '$T/out/program.calls'"
reset_out
printf 'libjxl.so.0.12\tffmpeg-legacy-580xx\nlibplacebo.so.360\tffmpeg-legacy-580xx\n' > "$kept_list"
cp "$T/files.all" "$T/files"
PACMAN_RC=1 run "$me" > "$T/out/run.txt" 2>&1
check "інший стан бібліотек — лагодить знову" "grep -qx -- '--repair' '$T/out/program.calls'"
check "pacman -U не пройшов — сповіщає про невдачу" "grep -q -- '--auto-update-notice failed' '$T/out/program.calls'"

reset_out
touch "$T/db.lck"
run "$me" > "$T/out/run.txt" 2>&1
check "pacman не звільнив базу — нічого не ставить" "[[ ! -e '$T/out/pacman.calls' && ! -e '$T/out/program.calls' ]]"
rm -f "$T/db.lck"

exec 8>&-
exec 7>>"$run_lock"; flock -n 7
reset_out
run_wait=1 run "$me" > "$T/out/run.txt" 2>&1
check "другий запуск чекає на перший і не ставить нічого сам" "grep -q 'previous run' '$T/out/run.txt' && [[ ! -e '$T/out/pacman.calls' ]]"
exec 7>&-

echo "use-package"
reset_out
( unset PKEXEC_UID DOAS_USER; SUDO_UID=$my_uid use_package ) > "$T/out/use.txt" 2>&1
check "без копії в ~/.local/bin нічого не робить" "[[ ! -e '$T/out/program.calls' ]]"
mkdir -p "$T/home/.local/bin" && touch "$T/home/.local/bin/obs-legacy"
( unset PKEXEC_UID DOAS_USER; SUDO_UID=$my_uid use_package ) > "$T/out/use.txt" 2>&1
check "копію від install.sh переймає програма з пакета" "grep -qx -- '--use-package' '$T/out/program.calls'"

echo
if ((fails)); then echo "НЕ ПРОЙДЕНО: $fails"; exit 1; fi
echo "Усе пройдено."
