#!/usr/bin/env bash
# Імітація скрипта збирання для розробки TUI без справжньої збірки OBS:
#
#   OBS_LEGACY_SCRIPT="$PWD/dev/fake-build.sh" cargo run
#
# Друкує те саме, що й справжній скрипт (етапи, [OK]/[УВАГА]/[ПОМИЛКА]/[ПОРАДА],
# запитання [y/N], «пароль», прогрес ninja і curl, «Звіт: …»), але нічого не змінює.
#   FAKE_FAIL=1 — завершитися помилкою на компіляції; FAKE_FAST=1 — без пауз.
#   «package» — те саме, що install, але без встановлення пакетів.
#   Відповідь «n» на запитання про пакети — скасування ([СКАСОВАНО], код 10).
#   «deps» ставить пакети понарошку: після нього check проходить без зауважень
#   (стан — у теці звітів, файл .fake-deps-installed; видали його, щоб почати знову).
#
# Звіти пишуться в $XDG_CACHE_HOME/obs-studio-nvenc-artix/reports, лише якщо
# XDG_CACHE_HOME задано явно, — щоб імітація не змішувалася зі справжніми
# звітами. Повний цикл зі списком звітів:
#   XDG_CACHE_HOME=/tmp/obs-legacy-fake-cache OBS_LEGACY_SCRIPT="$PWD/dev/fake-build.sh" cargo run
set -Eeuo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
real="$here/../scripts/build-obs-nvenc-legacy-artix.sh"
case "${1:-}" in
  plugins|help|version) exec bash "$real" "$@" ;;
esac

C_RED=$'\e[31m' C_GRN=$'\e[32m' C_YEL=$'\e[33m' C_BLU=$'\e[34m' C_CYN=$'\e[36m' C_BLD=$'\e[1m' C_RST=$'\e[0m'
say()  { printf '%s==>%s %s\n' "$C_BLU$C_BLD" "$C_RST" "$*" >&2; }
ok()   { printf '%s[OK]%s %s\n' "$C_GRN" "$C_RST" "$*" >&2; }
warn() { printf '%s[УВАГА]%s %s\n' "$C_YEL" "$C_RST" "$*" >&2; }
fail() { printf '%s[ПОМИЛКА]%s %s\n' "$C_RED" "$C_RST" "$*" >&2; }
hint() { printf '%s[ПОРАДА]%s %s\n' "$C_CYN" "$C_RST" "$*" >&2; }
fix()  { printf '%s[ВИПРАВИТИ]%s %s: %s\n' "$C_GRN" "$C_RST" "$1" "$2" >&2; }
cancelled() { printf '%s[СКАСОВАНО]%s %s\n' "$C_YEL" "$C_RST" "$*" >&2; exit 10; }
nap()  { if [[ "${FAKE_FAST:-0}" != 1 ]]; then sleep "$1"; fi; }
confirm() {
  local answer=''
  printf '%s [y/N]: ' "$1" >&2
  read -r answer || return 1
  [[ "$answer" =~ ^([yY]|[yY][eE][sS]|т|Т|так|Так)$ ]]
}

ninja() {   # $1 = кількість кроків, $2 = опис
  local i total=$1
  for ((i = 1; i <= total; i++)); do
    printf '[%d/%d] Building CXX object %s/file%03d.cpp.o\n' "$i" "$total" "$2" "$i"
    nap 0.03
  done
}

fake() {
  local cmd=$1
  printf '\n%sімітація %s — %s%s\n' "$C_BLD" "build-obs-nvenc-legacy-artix" "$cmd" "$C_RST" >&2
  say "Система"
  printf '  ОС: Artix Linux; ядро 6.17.5-artix1-1; init: runit\n  CPU: 8 потоків; доступно RAM: 14 GiB\n' >&2
  nap 0.4
  say "NVIDIA"
  printf '  GPU, драйвер: NVIDIA GeForce GTX 1060 6GB, 580.95.05\n  Драйвер підтримує NVENC API до: 13.0\n' >&2
  nap 0.4
  # update і rebuild — як install (FAKE_NOTHING_NEW=1: «оновлень немає»).
  local mode=$cmd
  if [[ "$cmd" == update || "$cmd" == rebuild ]]; then cmd=install; fi
  if [[ "$cmd" == check || "$cmd" == install || "$cmd" == package ]]; then
    say "Оптимізація під залізо"
    printf '  Дистрибутив: Artix Linux (artix) — базові прапорці з makepkg.conf, як у пакетів з репозиторію\n' >&2
    printf '  Процесор: Intel(R) Core(TM) i7-6700 CPU @ 3.40GHz → під цей процесор: skylake\n' >&2
    printf '  Розширення: SSE4.2 AVX AVX2 FMA · найвищий рівень ISA: x86-64-v3\n' >&2
    printf '  Відеокарта: NVIDIA GeForce GTX 1060 6GB (Pascal, CUDA 6.1) · драйвер 580.95.05 (nvidia-580xx-utils)\n' >&2
    printf '  C/C++: -march=native -mtune=native -O2 -pipe -fno-plt -fexceptions -Wp,-D_FORTIFY_SOURCE=3\n' >&2
    ok "Профіль: skylake · -O2 · CUDA sm_61 — усе збирається саме під цей комп'ютер."
    nap 0.3
  fi

  case "$cmd" in
    status|clean|rollback|uninstall)
      say "Стан ($cmd)"
      ok "Нічого не змінено — це імітація."
      return 0
      ;;
  esac

  say "Пакети для збірки"
  if [[ "$cmd" == deps ]]; then
    # Як install_dependencies: без попереджень, одразу список і запитання.
    if [[ -f "$deps_state" ]]; then
      ok "Усі потрібні пакети вже встановлені."
      return 0
    fi
    printf 'Буде встановлено (pacman -S --needed, БЕЗ оновлення системи):\n  libdatachannel\n  srt\n' >&2
    confirm "Встановити ці пакети?" || cancelled "Нічого не змінено."
    printf '[sudo] password for %s: ' "${USER:-user}" >&2
    read -rs _ || true
    printf '\n' >&2
    : > "$deps_state"
    ok "Пакети встановлено."
    printf '\n%sПАКЕТИ ГОТОВІ%s: усе потрібне для збірки встановлено.\n' "$C_GRN$C_BLD" "$C_RST" >&2
    hint "Тепер перевір систему ще раз («Перевірити систему») — усе має бути ✓."
    return 0
  fi
  if [[ -f "$deps_state" ]]; then
    ok "Усі потрібні пакети вже встановлені."
  else
    warn "Ще не встановлено: libdatachannel srt"
    hint "Постав їх зараз кнопкою «Виправити» — лише ці пакети, без оновлення системи. Або нічого не роби: їх поставить «Встановити / оновити OBS»."
    fix deps "Встановити пакети: libdatachannel srt"
  fi
  if [[ "$cmd" == install && ! -f "$deps_state" ]]; then
    confirm "Встановити ці пакети?" \
      || cancelled "Без цих пакетів OBS не зібрати, тому нічого не змінено. Запусти встановлення ще раз, коли будеш готовий."
    printf '[sudo] password for %s: ' "${USER:-user}" >&2
    read -rs _ || true
    printf '\n' >&2
    ok "Пакети встановлено."
  fi

  say "nv-codec-headers (приватна копія в кеші, системні файли не змінюються)"
  printf '  Драйвер знає NVENC API 13.0 → найновіші сумісні заголовки: n13.0.19.1\n' >&2
  ok "Заголовки n13.0.19.1 (13.0.19.1.2) готові: NVENC API 13.0."
  say "CEF (Chromium Embedded Framework) для Browser Source"
  say "Завантажую https://cdn-fastly.obsproject.com/downloads/cef_binary_7871_linux_x86_64.tar.xz"
  for p in $(seq 0 4 100); do
    printf '\r%-60s %5.1f%%' "$(head -c $((p * 60 / 100)) /dev/zero | tr '\0' '#')" "$p" >&2
    nap 0.04
  done
  printf '\n' >&2
  ok "SHA-256 збігається з офіційним хешем OBS."
  say "Плагіни"
  printf '  obs-multi-rtmp: буде зібрано 0.8.0.0 (0123456789ab)\n' >&2
  if [[ "$mode" == update ]]; then
    say "Що нового"
    if [[ "${FAKE_NOTHING_NEW:-0}" == 1 ]]; then
      ok "Усе вже найновіше: OBS 33.0.1; плагіни: obs-multi-rtmp 0.8.0.0."
      printf '\n%sОНОВЛЕНЬ НЕМАЄ%s: усе вже найновіше, збирати й ставити нічого.\n' "$C_GRN$C_BLD" "$C_RST" >&2
      printf '  Якщо OBS поводиться дивно або минуле збирання обірвалося — «Перезібрати OBS».\n' >&2
      printf 'Підсумок: оновлень немає — встановлено найновіше: OBS 33.0.1; плагіни: obs-multi-rtmp 0.8.0.0\n' >&2
      return 0
    fi
    printf '  OBS 33.0.0 → 33.0.1\n' >&2
  fi

  if [[ "$cmd" != install && "$cmd" != package ]]; then
    say "Конфігурація CMake (лише перевірка)"
    if [[ -f "$deps_state" ]]; then
      ok "CMake: Browser Source, власний NVENC (SDK 13.0), CEF 7871."
      printf '\n%sПЕРЕВІРКУ ПРОЙДЕНО%s: можна встановлювати.\n' "$C_GRN$C_BLD" "$C_RST" >&2
      return 0
    fi
    warn "Пробну конфігурацію пропущено: спершу потрібні пакети libdatachannel srt."
    fix deps "Встановити пакети: libdatachannel srt"
    printf '\n%sПЕРЕВІРКУ ЗАВЕРШЕНО%s: усе готово до встановлення, бракує лише пакетів з репозиторіїв.\n' "$C_YEL$C_BLD" "$C_RST" >&2
    hint "Постав їх кнопкою «Виправити» і перевір систему ще раз — тоді все буде ✓. Або одразу обери «Встановити / оновити OBS»: він теж їх поставить."
    return 0
  fi

  say "Компіляція OBS (4 потоків, знижений пріоритет). На старішому залізі це може тривати годину й більше."
  ninja 240 libobs
  if [[ "${FAKE_FAIL:-0}" == 1 ]]; then
    fail "Компіляція не вдалася (див. вивід вище і звіт). Нічого не встановлено і не змінено."
    hint "Перезбери начисто: «Перезібрати OBS» — це допомагає, якщо попереднє збирання обірвалося чи кеш зіпсовано. Якщо помилка повториться — створи issue на GitHub і додай звіт."
    fix rebuild "Перезібрати начисто"
    return 1
  fi
  ok "Компіляція завершена."
  say "Перевіряю NVENC тестом OBS: obs-nvenc-test"
  printf '  nvenc_supported=true\n  nvenc_ver=13.0\n' >&2
  ok "NVENC працює з цією збіркою (nvenc_ver=13.0)."
  say "Плагін obs-multi-rtmp (Multiple RTMP Outputs)"
  ninja 18 obs-multi-rtmp
  ok "Плагін obs-multi-rtmp зібрано."
  say "Перевіряю бібліотеки (ldd)"
  ok "Усі бібліотеки знаходяться."
  say "Пакети pacman: obs-studio-legacy-580xx і плагіни (-legacy-580xx)"
  ok "Пакет: obs-studio-legacy-580xx-33.0.0-1-x86_64.pkg.tar.zst (231M)"
  ok "Пакет: obs-multi-rtmp-legacy-580xx-0.8.0.0-1-x86_64.pkg.tar.zst"
  ok "Набір пакетів готовий: ~/.local/share/obs-studio-nvenc-artix/packages/20261029-180000_33.0.0"
  if [[ "$cmd" == package ]]; then
    printf '\n%sПАКЕТИ ГОТОВІ%s: OBS 33.0.0 зібрано під цей комп'"'"'ютер і запаковано (імітація).\n' "$C_GRN$C_BLD" "$C_RST" >&2
    return 0
  fi
  say "Встановлення пакетів (pacman -U)"
  printf '  Буде замінено (pacman спитає — відповідай y): obs-studio-git\n' >&2
  confirm "Встановити пакети OBS-Legacy?" || cancelled "Пакети збережено — їх можна поставити пізніше."
  printf ':: obs-studio-legacy-580xx-33.0.0-1 і obs-studio-git-33.0.0.r3-1 конфліктують (obs-studio). Видалити obs-studio-git? [y/N] ' >&2
  read -r _ || true
  ok "Пакети встановлено."
  printf '\n%sГОТОВО%s: OBS 33.0.0 зібрано під цей комп'"'"'ютер, NVENC перевірено, пакети встановлено (імітація).\n' "$C_GRN$C_BLD" "$C_RST" >&2
  case "$mode" in
    update)  printf 'Підсумок: OBS 33.0.0 → 33.0.1\n' >&2 ;;
    rebuild) printf 'Підсумок: OBS 33.0.1 перезібрано начисто\n' >&2 ;;
    *)       printf 'Підсумок: встановлено OBS 33.0.1 з NVENC\n' >&2 ;;
  esac
}

cmd=${1:-install}
if [[ -n "${XDG_CACHE_HOME:-}" ]]; then
  reports="$XDG_CACHE_HOME/obs-studio-nvenc-artix/reports"
else
  reports="${TMPDIR:-/tmp}/obs-legacy-fake/reports"
fi
mkdir -p -- "$reports"
deps_state="$reports/.fake-deps-installed"
# Для TUI: яких пакетів бракує (як «deps --missing» справжнього скрипта).
if [[ "$cmd" == deps && "${2:-}" == --missing ]]; then
  if [[ ! -f "$deps_state" ]]; then printf 'repo libdatachannel\nrepo srt\n'; fi
  exit 0
fi
report="$reports/$(date +%Y%m%d-%H%M%S)-$cmd.txt"
# Як run_logged у справжньому скрипті: вивід і в термінал, і у звіт (без кольорів).
set +e
( fake "$cmd" ) 2>&1 | tee -- "$report.raw"
rc=${PIPESTATUS[0]}
set -e
sed -E 's/\x1B\[[0-9;]*[A-Za-z]//g; s/^.*\r//' -- "$report.raw" > "$report"
rm -f -- "$report.raw"
case "$rc" in
  0) printf '\nРезультат: успішно\n' ;;
  10) printf '\nРезультат: скасовано\n' ;;
  *) printf '\nРезультат: помилка (код %s)\n' "$rc" ;;
esac >> "$report"
# Шлях у домашній теці — через «~» (TUI розгортає його сам): так у
# скріншотах не видно імені користувача.
printf '\nЗвіт: %s\n' "${report/#"$HOME"\//\~/}" >&2
exit "$rc"
