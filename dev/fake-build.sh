#!/usr/bin/env bash
# shellcheck disable=SC1111  # типографські лапки “…” в англійських повідомленнях — навмисні
# Імітація скрипта збирання для розробки TUI без справжньої збірки OBS:
#
#   OBS_LEGACY_SCRIPT="$PWD/dev/fake-build.sh" cargo run
#
# Друкує те саме, що й справжній скрипт, і тією самою мовою (OBS_LEGACY_LANG=uk|en, інакше
# локаль): етапи, [OK]/[УВАГА]/[ПОМИЛКА]/[ПОРАДА] (англійською — [WARNING]/[ERROR]/[HINT]),
# запитання [y/N], «пароль», прогрес ninja і curl, «Звіт: …» — але нічого не змінює.
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

# Мова — як у справжнього скрипта: OBS_LEGACY_LANG (його передає TUI), інакше локаль.
case "${OBS_LEGACY_LANG:-}" in
  uk) LANG_EN=0 ;;
  en) LANG_EN=1 ;;
  *) if [[ "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" == uk* ]]; then LANG_EN=0; else LANG_EN=1; fi ;;
esac
t() { if ((LANG_EN)); then printf '%s' "$2"; else printf '%s' "$1"; fi; }

C_RED=$'\e[31m' C_GRN=$'\e[32m' C_YEL=$'\e[33m' C_BLU=$'\e[34m' C_CYN=$'\e[36m' C_BLD=$'\e[1m' C_RST=$'\e[0m'
say()  { printf '%s==>%s %s\n' "$C_BLU$C_BLD" "$C_RST" "$*" >&2; }
ok()   { printf '%s[OK]%s %s\n' "$C_GRN" "$C_RST" "$*" >&2; }
warn() { printf '%s[%s]%s %s\n' "$C_YEL" "$(t УВАГА WARNING)" "$C_RST" "$*" >&2; }
fail() { printf '%s[%s]%s %s\n' "$C_RED" "$(t ПОМИЛКА ERROR)" "$C_RST" "$*" >&2; }
hint() { printf '%s[%s]%s %s\n' "$C_CYN" "$(t ПОРАДА HINT)" "$C_RST" "$*" >&2; }
fix()  { printf '%s[%s]%s %s: %s\n' "$C_GRN" "$(t ВИПРАВИТИ FIX)" "$C_RST" "$1" "$2" >&2; }
cancelled() { printf '%s[%s]%s %s\n' "$C_YEL" "$(t СКАСОВАНО CANCELLED)" "$C_RST" "$*" >&2; exit 10; }
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
  printf "$(t '\n%sімітація %s — %s%s\n' \
              '\n%ssimulation %s — %s%s\n')" "$C_BLD" "build-obs-nvenc-legacy-artix" "$cmd" "$C_RST" >&2
  say "$(t "Система" "System")"
  printf "$(t '  ОС: Artix Linux; ядро 6.17.5-artix1-1; init: runit\n  CPU: 8 потоків; доступно RAM: 14 GiB\n' \
              '  OS: Artix Linux; kernel 6.17.5-artix1-1; init: runit\n  CPU: 8 threads; RAM available: 14 GiB\n')" >&2
  nap 0.4
  say "NVIDIA"
  printf "$(t '  GPU, драйвер: NVIDIA GeForce GTX 1060 6GB, 580.95.05\n  Драйвер підтримує NVENC API до: 13.0\n' \
              '  GPU, driver: NVIDIA GeForce GTX 1060 6GB, 580.95.05\n  The driver supports NVENC API up to: 13.0\n')" >&2
  nap 0.4
  # update і rebuild — як install (FAKE_NOTHING_NEW=1: «оновлень немає»).
  local mode=$cmd
  if [[ "$cmd" == update || "$cmd" == rebuild ]]; then cmd=install; fi
  if [[ "$cmd" == check || "$cmd" == install || "$cmd" == package ]]; then
    say "$(t "Оптимізація під залізо" "Hardware optimization")"
    printf "$(t '  Дистрибутив: Artix Linux (artix) — базові прапорці з makepkg.conf, як у пакетів з репозиторію\n' \
                '  Distribution: Artix Linux (artix) — base flags from makepkg.conf, like the repository packages\n')" >&2
    printf "$(t '  Процесор: Intel(R) Core(TM) i7-6700 CPU @ 3.40GHz → під цей процесор: skylake\n' \
                '  CPU: Intel(R) Core(TM) i7-6700 CPU @ 3.40GHz → for this CPU: skylake\n')" >&2
    printf "$(t '  Розширення: SSE4.2 AVX AVX2 FMA · найвищий рівень ISA: x86-64-v3\n' \
                '  Extensions: SSE4.2 AVX AVX2 FMA · highest ISA level: x86-64-v3\n')" >&2
    printf "$(t '  Відеокарта: NVIDIA GeForce GTX 1060 6GB (Pascal, CUDA 6.1) · драйвер 580.95.05 (nvidia-580xx-utils)\n' \
                '  GPU: NVIDIA GeForce GTX 1060 6GB (Pascal, CUDA 6.1) · driver 580.95.05 (nvidia-580xx-utils)\n')" >&2
    printf '  C/C++: -march=native -mtune=native -O2 -pipe -fno-plt -fexceptions -Wp,-D_FORTIFY_SOURCE=3\n' >&2
    ok "$(t "Профіль: skylake · -O2 · CUDA sm_61 — усе збирається саме під цей комп'ютер." \
            "Profile: skylake · -O2 · CUDA sm_61 — everything is built for exactly this computer.")"
    nap 0.3
  fi

  case "$cmd" in
    status|clean|rollback|uninstall)
      say "$(t "Стан ($cmd)" "Status ($cmd)")"
      ok "$(t "Нічого не змінено — це імітація." "Nothing was changed — this is a simulation.")"
      return 0
      ;;
  esac

  say "$(t "Пакети для збірки" "Build dependencies")"
  if [[ "$cmd" == deps ]]; then
    # Як install_dependencies: без попереджень, одразу список і запитання.
    if [[ -f "$deps_state" ]]; then
      ok "$(t "Усі потрібні пакети вже встановлені." "All required packages are already installed.")"
      return 0
    fi
    printf "$(t 'Буде встановлено (pacman -S --needed, БЕЗ оновлення системи):\n  libdatachannel\n  srt\n' \
                'Will be installed (pacman -S --needed, WITHOUT a system upgrade):\n  libdatachannel\n  srt\n')" >&2
    confirm "$(t "Встановити ці пакети?" \
                 "Install these packages?")" || cancelled "$(t "Нічого не змінено." "Nothing was changed.")"
    printf '[sudo] password for %s: ' "${USER:-user}" >&2
    read -rs _ || true
    printf '\n' >&2
    : > "$deps_state"
    ok "$(t "Пакети встановлено." "Packages installed.")"
    printf "$(t '\n%sПАКЕТИ ГОТОВІ%s: усе потрібне для збірки встановлено.\n' \
                '\n%sPACKAGES READY%s: everything needed for the build is installed.\n')" "$C_GRN$C_BLD" "$C_RST" >&2
    hint "$(t "Тепер перевір систему ще раз («Перевірити систему») — усе має бути ✓." \
              "Now check the system again (“Check system”) — everything should be ✓.")"
    return 0
  fi
  if [[ -f "$deps_state" ]]; then
    ok "$(t "Усі потрібні пакети вже встановлені." "All required packages are already installed.")"
  else
    warn "$(t "Ще не встановлено: libdatachannel srt" "Not installed yet: libdatachannel srt")"
    hint "$(t "Постав їх зараз кнопкою «Виправити» — лише ці пакети, без оновлення системи. Або нічого не роби: їх поставить «Встановити / оновити OBS»." \
              "Install them now with the “Fix” button — only these packages, without a system upgrade. Or do nothing: “Install / update OBS” will install them.")"
    fix deps "$(t "Встановити пакети: libdatachannel srt" "Install packages: libdatachannel srt")"
  fi
  if [[ "$cmd" == install && ! -f "$deps_state" ]]; then
    confirm "$(t "Встановити ці пакети?" "Install these packages?")" \
      || cancelled "$(t "Без цих пакетів OBS не зібрати, тому нічого не змінено. Запусти встановлення ще раз, коли будеш готовий." \
                        "OBS cannot be built without these packages, so nothing was changed. Start the installation again when you are ready.")"
    printf '[sudo] password for %s: ' "${USER:-user}" >&2
    read -rs _ || true
    printf '\n' >&2
    ok "$(t "Пакети встановлено." "Packages installed.")"
  fi

  say "$(t "nv-codec-headers (приватна копія в кеші, системні файли не змінюються)" \
           "nv-codec-headers (a private copy in the cache, system files are not changed)")"
  printf "$(t '  Драйвер знає NVENC API 13.0 → найновіші сумісні заголовки: n13.0.19.1\n' \
              '  The driver knows NVENC API 13.0 → the newest compatible headers: n13.0.19.1\n')" >&2
  ok "$(t "Заголовки n13.0.19.1 (13.0.19.1.2) готові: NVENC API 13.0." \
          "Headers n13.0.19.1 (13.0.19.1.2) are ready: NVENC API 13.0.")"
  say "$(t "CEF (Chromium Embedded Framework) для Browser Source" \
           "CEF (Chromium Embedded Framework) for Browser Source")"
  say "$(t "Завантажую https://cdn-fastly.obsproject.com/downloads/cef_binary_7871_linux_x86_64.tar.xz" \
           "Downloading https://cdn-fastly.obsproject.com/downloads/cef_binary_7871_linux_x86_64.tar.xz")"
  for p in $(seq 0 4 100); do
    printf '\r%-60s %5.1f%%' "$(head -c $((p * 60 / 100)) /dev/zero | tr '\0' '#')" "$p" >&2
    nap 0.04
  done
  printf '\n' >&2
  ok "$(t "SHA-256 збігається з офіційним хешем OBS." "SHA-256 matches the official OBS hash.")"
  say "$(t "Плагіни" "Plugins")"
  printf "$(t '  obs-multi-rtmp: буде зібрано 0.8.0.0 (0123456789ab)\n' \
              '  obs-multi-rtmp: 0.8.0.0 will be built (0123456789ab)\n')" >&2
  if [[ "$mode" == update ]]; then
    say "$(t "Що нового" "What's new")"
    if [[ "${FAKE_NOTHING_NEW:-0}" == 1 ]]; then
      ok "$(t "Усе вже найновіше: OBS 33.0.1; плагіни: obs-multi-rtmp 0.8.0.0." \
              "Everything is up to date: OBS 33.0.1; plugins: obs-multi-rtmp 0.8.0.0.")"
      printf "$(t '\n%sОНОВЛЕНЬ НЕМАЄ%s: усе вже найновіше, збирати й ставити нічого.\n' \
                  '\n%sNO UPDATES%s: everything is up to date, nothing to build or install.\n')" "$C_GRN$C_BLD" "$C_RST" >&2
      printf "$(t '  Якщо OBS поводиться дивно або минуле збирання обірвалося — «Перезібрати OBS».\n' \
                  '  If OBS behaves strangely or the last build was interrupted — “Rebuild OBS”.\n')" >&2
      printf "$(t 'Підсумок: оновлень немає — встановлено найновіше: OBS 33.0.1; плагіни: obs-multi-rtmp 0.8.0.0\n' \
                  'Summary: no updates — the newest is installed: OBS 33.0.1; plugins: obs-multi-rtmp 0.8.0.0\n')" >&2
      return 0
    fi
    printf '  OBS 33.0.0 → 33.0.1\n' >&2
  fi

  if [[ "$cmd" != install && "$cmd" != package ]]; then
    say "$(t "Конфігурація CMake (лише перевірка)" "CMake configuration (check only)")"
    if [[ -f "$deps_state" ]]; then
      ok "$(t "CMake: Browser Source, власний NVENC (SDK 13.0), CEF 7871." \
              "CMake: Browser Source, own NVENC (SDK 13.0), CEF 7871.")"
      printf "$(t '\n%sПЕРЕВІРКУ ПРОЙДЕНО%s: можна встановлювати.\n' \
                  '\n%sCHECK PASSED%s: ready to install.\n')" "$C_GRN$C_BLD" "$C_RST" >&2
      return 0
    fi
    warn "$(t "Пробну конфігурацію пропущено: спершу потрібні пакети libdatachannel srt." \
              "The test configuration was skipped: the packages libdatachannel srt are needed first.")"
    fix deps "$(t "Встановити пакети: libdatachannel srt" "Install packages: libdatachannel srt")"
    printf "$(t '\n%sПЕРЕВІРКУ ЗАВЕРШЕНО%s: усе готово до встановлення, бракує лише пакетів з репозиторіїв.\n' \
                '\n%sCHECK FINISHED%s: everything is ready for installation, only packages from the repositories are missing.\n')" "$C_YEL$C_BLD" "$C_RST" >&2
    hint "$(t "Постав їх кнопкою «Виправити» і перевір систему ще раз — тоді все буде ✓. Або одразу обери «Встановити / оновити OBS»: він теж їх поставить." \
              "Install them with the “Fix” button and check the system again — then everything will be ✓. Or choose “Install / update OBS” right away: it installs them too.")"
    return 0
  fi

  say "$(t "Компіляція OBS (4 потоків, знижений пріоритет). На старішому залізі це може тривати годину й більше." \
           "Compiling OBS (4 jobs, lowered priority). On older hardware this can take an hour or more.")"
  ninja 240 libobs
  if [[ "${FAKE_FAIL:-0}" == 1 ]]; then
    fail "$(t "Компіляція не вдалася (див. вивід вище і звіт). Нічого не встановлено і не змінено." \
              "Compilation failed (see the output above and the report). Nothing was installed or changed.")"
    hint "$(t "Перезбери начисто: «Перезібрати OBS» — це допомагає, якщо попереднє збирання обірвалося чи кеш зіпсовано. Якщо помилка повториться — створи issue на GitHub і додай звіт." \
              "Rebuild from scratch: “Rebuild OBS” — this helps if the previous build was interrupted or the cache is damaged. If the error comes back, open an issue on GitHub and attach the report.")"
    fix rebuild "$(t "Перезібрати начисто" "Rebuild from scratch")"
    return 1
  fi
  ok "$(t "Компіляція завершена." "Compilation finished.")"
  say "$(t "Перевіряю NVENC тестом OBS: obs-nvenc-test" "Testing NVENC with the OBS test: obs-nvenc-test")"
  printf '  nvenc_supported=true\n  nvenc_ver=13.0\n' >&2
  ok "$(t "NVENC працює з цією збіркою (nvenc_ver=13.0)." "NVENC works with this build (nvenc_ver=13.0).")"
  say "$(t "Плагін obs-multi-rtmp (Multiple RTMP Outputs)" "Plugin obs-multi-rtmp (Multiple RTMP Outputs)")"
  ninja 18 obs-multi-rtmp
  ok "$(t "Плагін obs-multi-rtmp зібрано." "Plugin obs-multi-rtmp built.")"
  say "$(t "Перевіряю бібліотеки (ldd)" "Checking libraries (ldd)")"
  ok "$(t "Усі бібліотеки знаходяться." "All libraries are found.")"
  say "$(t "Пакети pacman: obs-studio-legacy-580xx і плагіни (-legacy-580xx)" \
           "pacman packages: obs-studio-legacy-580xx and plugins (-legacy-580xx)")"
  ok "$(t "Пакет: obs-studio-legacy-580xx-33.0.0-1-x86_64.pkg.tar.zst (231M)" \
          "Package: obs-studio-legacy-580xx-33.0.0-1-x86_64.pkg.tar.zst (231M)")"
  ok "$(t "Пакет: obs-multi-rtmp-legacy-580xx-0.8.0.0-1-x86_64.pkg.tar.zst" \
          "Package: obs-multi-rtmp-legacy-580xx-0.8.0.0-1-x86_64.pkg.tar.zst")"
  ok "$(t "Набір пакетів готовий: ~/.local/share/obs-studio-nvenc-artix/packages/20261029-180000_33.0.0" \
          "The package set is ready: ~/.local/share/obs-studio-nvenc-artix/packages/20261029-180000_33.0.0")"
  if [[ "$cmd" == package ]]; then
    printf "$(t "\n%sПАКЕТИ ГОТОВІ%s: OBS 33.0.0 зібрано під цей комп'ютер і запаковано (імітація).\n" \
                "\n%sPACKAGES READY%s: OBS 33.0.0 is built for this computer and packed (simulation).\n")" "$C_GRN$C_BLD" "$C_RST" >&2
    return 0
  fi
  say "$(t "Встановлення пакетів (pacman -U)" "Installing the packages (pacman -U)")"
  printf "$(t '  Буде замінено (pacman спитає — відповідай y): obs-studio-git\n' \
              '  Will be replaced (pacman will ask — answer y): obs-studio-git\n')" >&2
  confirm "$(t "Встановити пакети OBS-Legacy?" \
               "Install the OBS-Legacy packages?")" || cancelled "$(t "Пакети збережено — їх можна поставити пізніше." \
                                                                      "The packages are saved — you can install them later.")"
  printf "$(t ':: obs-studio-legacy-580xx-33.0.0-1 і obs-studio-git-33.0.0.r3-1 конфліктують (obs-studio). Видалити obs-studio-git? [y/N] ' \
              ':: obs-studio-legacy-580xx-33.0.0-1 and obs-studio-git-33.0.0.r3-1 are in conflict (obs-studio). Remove obs-studio-git? [y/N] ')" >&2
  read -r _ || true
  ok "$(t "Пакети встановлено." "Packages installed.")"
  printf "$(t "\n%sГОТОВО%s: OBS 33.0.0 зібрано під цей комп'ютер, NVENC перевірено, пакети встановлено (імітація).\n" \
              "\n%sDONE%s: OBS 33.0.0 is built for this computer, NVENC is tested, the packages are installed (simulation).\n")" \
    "$C_GRN$C_BLD" "$C_RST" >&2
  case "$mode" in
    update)  printf "$(t 'Підсумок: OBS 33.0.0 → 33.0.1\n' 'Summary: OBS 33.0.0 → 33.0.1\n')" >&2 ;;
    rebuild) printf "$(t 'Підсумок: OBS 33.0.1 перезібрано начисто\n' \
                         'Summary: OBS 33.0.1 rebuilt from scratch\n')" >&2 ;;
    *)       printf "$(t 'Підсумок: встановлено OBS 33.0.1 з NVENC\n' \
                         'Summary: installed OBS 33.0.1 with NVENC\n')" >&2 ;;
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
  0) printf "$(t '\nРезультат: успішно\n' '\nResult: success\n')" ;;
  10) printf "$(t '\nРезультат: скасовано\n' '\nResult: cancelled\n')" ;;
  *) printf "$(t '\nРезультат: помилка (код %s)\n' '\nResult: error (code %s)\n')" "$rc" ;;
esac >> "$report"
# Шлях у домашній теці — через «~» (TUI розгортає його сам): так у
# скріншотах не видно імені користувача.
printf "$(t '\nЗвіт: %s\n' '\nReport: %s\n')" "${report/#"$HOME"\//\~/}" >&2
exit "$rc"
