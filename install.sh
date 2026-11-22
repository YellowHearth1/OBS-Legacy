#!/usr/bin/env bash
# shellcheck disable=SC1111  # типографські лапки “…” в англійських повідомленнях — навмисні
# =============================================================================
#  install.sh — встановлює програму OBS-Legacy для поточного користувача
#  https://github.com/YellowHearth1/OBS-Legacy
# -----------------------------------------------------------------------------
#  Створено за допомогою Claude Code (ШІ-асистент Anthropic) на замовлення і
#  під керівництвом автора репозиторію. Натхнення:
#  https://github.com/sl5net/build-obs-nvenc-legacy
#
#  Що робить:
#    * без теки з вихідними кодами (curl … | bash) на Arch і Artix ставить пакет
#      pacman obs-legacy з релізу на GitHub: перевіряє його суму SHA-256 і ставить
#      локальним файлом (sudo pacman -U). З пакетом OBS-Legacy оновлює свої пакети
#      разом із системою. Копію, яку install.sh колись поклав у ~/.local/bin,
#      пакет переймає сам;
#    * без pacman або з --no-package кладе ту саму готову програму в ~/.local/bin
#      (без sudo, без Rust і без збирання);
#    * у теці з вихідними кодами (git clone) збирає obs-legacy під цей процесор
#      (cargo build --release --locked) у ~/.local/bin, а після встановлення
#      прибирає проміжні файли збирання (target/, сотні мегабайтів);
#    * додає ярлик «OBS-Legacy» у меню програм та іконку (у пакеті вони свої), за
#      бажанням — ярлик на робочий стіл. Клацання по ярлику одразу відкриває вікно
#      термінала з меню OBS-Legacy;
#    * за бажанням вмикає сповіщення про нові версії OBS, плагінів і самого
#      OBS-Legacy: службу користувача systemd, dinit, OpenRC, runit чи s6 або
#      автозапуск сесії (obs-legacy --notifier, без sudo).
#  У систему ставить лише пакет obs-legacy і (з твоєї згоди) пакет rust для збирання.
#
#  Використання:
#    curl -fsSL https://github.com/YellowHearth1/OBS-Legacy/releases/latest/download/install.sh | bash
#    ./install.sh              встановити або оновити (запитає про ярлик на столі)
#    ./install.sh --desktop    те саме + ярлик на робочому столі без запитання
#    ./install.sh --no-desktop без ярлика на робочому столі
#    ./install.sh --prebuilt   готова програма з релізу, навіть у теці з вихідними кодами
#    ./install.sh --no-package готова програма в ~/.local/bin, без пакета pacman і sudo
#    ./install.sh --update     без запитань (так його викликає сам obs-legacy)
#    ./install.sh --uninstall  прибрати програму й сповіщення (зібраний OBS не чіпає)
#  Параметри разом із curl: … | bash -s -- --uninstall
# =============================================================================
set -Eeuo pipefail

REPO_URL="https://github.com/YellowHearth1/OBS-Legacy"
# Реліз, з якого брати готову програму; порожньо — найновіший. У копію install.sh,
# яку робочий процес Package прикріплює до релізу, підставляється тег цього релізу.
RELEASE_TAG="${OBS_LEGACY_TAG:-}"
ASSET="obs-legacy-x86_64"
# Пакет pacman obs-legacy (packaging/PKGBUILD): програма з нього і її версія в назві файлу релізу
# (obs-legacy-<версія>-1-x86_64.pkg.tar.zst, так його називає .github/workflows/package.yml).
PKG_EXE=/usr/bin/obs-legacy
# Готову програму ставити пакетом там, де є pacman; 0 — копією в ~/.local/bin (--no-package).
USE_PACKAGE=1

# Тека з вихідними кодами — лише коли install.sh лежить поруч із Cargo.toml. Під
# «curl … | bash» файлу скрипта немає взагалі: тоді програма — готова, з релізу.
SRC_DIR=''
if [[ -f "${BASH_SOURCE[0]:-}" ]]; then
  SRC_DIR=$(cd -- "$(dirname -- "$(readlink -f -- "${BASH_SOURCE[0]}")")" && pwd)
  [[ -f "$SRC_DIR/Cargo.toml" && -d "$SRC_DIR/src" ]] || SRC_DIR=''
fi
TARGET_DIR=''
BIN_DIR="$HOME/.local/bin"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS_DIR="$DATA_HOME/applications"
ICON_DIR="$DATA_HOME/icons/hicolor/scalable/apps"
STATE_DIR="$DATA_HOME/obs-legacy"
DESKTOP_ID="obs-legacy.desktop"
readonly MARKER="Managed by OBS-Legacy install.sh"

TMP_DIR=''
cleanup() { if [[ -n "$TMP_DIR" ]]; then rm -rf -- "$TMP_DIR"; fi; }
trap cleanup EXIT

if [[ -t 2 ]]; then
  C_RED=$'\e[31m' C_GRN=$'\e[32m' C_YEL=$'\e[33m' C_BLU=$'\e[34m' C_BLD=$'\e[1m' C_RST=$'\e[0m'
else
  C_RED='' C_GRN='' C_YEL='' C_BLU='' C_BLD='' C_RST=''
fi
# Мова повідомлень — як у програми: OBS_LEGACY_LANG (його передає obs-legacy) → UI_LANG (змінна
# або налаштування) → локаль системи: uk… — українська, інакше англійська.
resolve_lang() {
  local v=${OBS_LEGACY_LANG:-} file=${OBS_LEGACY_SETTINGS:-${XDG_CONFIG_HOME:-$HOME/.config}/obs-legacy/settings.env} loc
  if [[ "$v" != uk && "$v" != en ]]; then
    v=${UI_LANG:-}
    if [[ -z "$v" && -r "$file" ]]; then
      # Перше входження ключа, як у TUI і скрипті збирання.
      v=$(sed -n '/^[[:space:]]*UI_LANG=/{s/^[[:space:]]*UI_LANG=//p;q;}' "$file" 2>/dev/null | tr -d "\"' " || true)
    fi
  fi
  if [[ "$v" != uk && "$v" != en ]]; then
    loc=${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}
    if [[ -z "$loc" && -r /etc/locale.conf ]]; then
      loc=$(sed -n 's/^LANG=//p' /etc/locale.conf 2>/dev/null | tr -d "\"'" | head -n1 || true)
    fi
    if [[ "$loc" == uk* ]]; then v=uk; else v=en; fi
  fi
  if [[ "$v" == en ]]; then LANG_EN=1; else LANG_EN=0; fi
}
resolve_lang
# Текст мовою повідомлень: t "українською" "in English".
t() { if ((LANG_EN)); then printf '%s' "$2"; else printf '%s' "$1"; fi; }

say()  { printf '%s==>%s %s\n' "$C_BLU$C_BLD" "$C_RST" "$*" >&2; }
ok()   { printf '%s[OK]%s %s\n' "$C_GRN" "$C_RST" "$*" >&2; }
warn() { printf '%s[%s]%s %s\n' "$C_YEL" "$(t УВАГА WARNING)" "$C_RST" "$*" >&2; }
die()  { printf '%s[%s]%s %s\n' "$C_RED" "$(t ПОМИЛКА ERROR)" "$C_RST" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# Чи можна питати (stdin — термінал); визначає main.
INTERACTIVE=0

# Відповідь без пробілів і керівних символів (коди стрілок, Esc). LC_ALL=C: в
# українській локалі діапазон [@-~] не охоплює латинських літер.
clean_answer() {
  local esc=$'\e'
  printf '%s' "$1" | LC_ALL=C sed -E "s|${esc}\\[[0-9;?]*[ -/]*[@-~]||g; s|${esc}O.||g; s|[[:cntrl:]]||g; s|[[:space:]]||g"
}

# Питання «так / ні». Незрозуміла відповідь — питання ще раз. «т» і «н» неоднозначні
# («так» / «ні», але й клавіші N і Y в українській розкладці) — підказка й ще раз.
ask_yes_no() {   # $1 = питання, $2 = «[y/N]» чи «[Y/n]», $3 = відповідь на Enter (0 — так, 1 — ні)
  local answer a
  (( INTERACTIVE )) || return 1
  while :; do
    printf '%s %s: ' "$1" "$2" >&2
    read -r answer || return 1
    a=$(clean_answer "$answer")
    case "$a" in
      '') return "$3" ;;
      [yY]|[yY][eE][sS]|так|Так|ТАК) return 0 ;;
      [nN]|[nN][oO]|ні|Ні|НІ) return 1 ;;
      т|Т|н|Н)
        printf "$(t '  «%s» — схоже, увімкнена українська розкладка (клавіша Y дає «н», N — «т»). Введи y чи n латиницею або напиши «так» / «ні».\n' \
                    '  “%s” — looks like the Ukrainian keyboard layout is on (the Y key gives “н”, N gives “т”). Type y or n in the Latin layout, or write “yes” / “no”.\n')" "$a" >&2 ;;
      *) printf "$(t '  Не зрозумів відповідь «%s». Введи y (так) або n (ні).\n' \
                     '  Did not understand the answer “%s”. Type y (yes) or n (no).\n')" "$a" >&2 ;;
    esac
  done
}

confirm() { ask_yes_no "$1" '[y/N]' 1; }       # Enter — ні. Без термінала — ні.
confirm_yes() { ask_yes_no "$1" '[Y/n]' 0; }   # Enter — так. Без термінала — ні.

# Атомарна заміна файлу: працює навіть тоді, коли obs-legacy зараз запущений.
put() {   # $1 = джерело, $2 = призначення, $3 = права
  local tmp
  mkdir -p -- "$(dirname -- "$2")"
  tmp=$(mktemp -- "$2.XXXXXX")
  cp -- "$1" "$tmp"
  chmod "$3" -- "$tmp"
  mv -f -- "$tmp" "$2"
}

# Завантажити файл: curl (є всюди, де є pacman) або wget. З'єднання, що дві хвилини не передає
# жодного байта (зник інтернет), обривається, і завантаження пробує ще раз, а не висить.
fetch() {   # $1 = URL, $2 = куди
  if have curl; then
    curl -fsSL --retry 3 --connect-timeout 20 --speed-limit 1 --speed-time 120 -o "$2" "$1"
  elif have wget; then
    wget -q --tries=3 --timeout=120 -O "$2" "$1"
  else
    die "$(t "Потрібен curl або wget." "curl or wget is needed.")"
  fi
}

# Файл із релізу. OBS_LEGACY_DOWNLOAD_URL — тека з тими самими файлами замість релізу
# (для перевірок, напр. file:///шлях): obs-legacy-x86_64, .sha256 і obs-legacy.svg.
release_url() {   # $1 = назва файлу
  if [[ -n "${OBS_LEGACY_DOWNLOAD_URL:-}" ]]; then
    printf '%s/%s\n' "${OBS_LEGACY_DOWNLOAD_URL%/}" "$1"
  elif [[ -n "$RELEASE_TAG" ]]; then
    printf '%s/releases/download/%s/%s\n' "$REPO_URL" "$RELEASE_TAG" "$1"
  else
    printf '%s/releases/latest/download/%s\n' "$REPO_URL" "$1"
  fi
}

# Іконка — з репозиторію на тому самому тезі (без тегу — з main), щоб у релізі не було зайвих файлів.
icon_url() {
  if [[ -n "${OBS_LEGACY_DOWNLOAD_URL:-}" ]]; then
    release_url obs-legacy.svg
  else
    printf 'https://raw.githubusercontent.com/YellowHearth1/OBS-Legacy/%s/assets/obs-legacy.svg\n' "${RELEASE_TAG:-main}"
  fi
}

desktop_dir() {
  local d=''
  if have xdg-user-dir; then d=$(xdg-user-dir DESKTOP 2>/dev/null || true); fi
  if [[ -z "$d" || "$d" == "$HOME" ]]; then d="$HOME/Desktop"; fi
  printf '%s\n' "$d"
}

write_desktop_entry() {   # $1 = файл, $2 = програма
  local exec_path=$2
  if [[ "$exec_path" == *' '* ]]; then exec_path="\"$exec_path\""; fi
  cat > "$1" <<EOF
[Desktop Entry]
# $MARKER
Type=Application
Name=OBS-Legacy
GenericName=OBS Studio with NVENC for NVIDIA 580xx
GenericName[uk]=OBS Studio з NVENC для NVIDIA 580xx
Comment=Build, install and update OBS Studio with NVENC for NVIDIA 580xx legacy drivers
Comment[uk]=Збирання, встановлення й оновлення OBS Studio з NVENC під драйвери NVIDIA 580xx
Exec=$exec_path
Icon=obs-legacy
Terminal=false
Categories=AudioVideo;Recorder;
Keywords=OBS;NVENC;NVIDIA;580;stream;record;
StartupNotify=false
EOF
}

# Сповіщення про оновлення: після оновлення програми — перезапустити фонову
# перевірку з новою версією; під час встановлення — запропонувати ввімкнути.
setup_notifier() {   # $1 = update|ask, $2 = програма
  local bin=$2
  if "$bin" --notifier is-enabled; then
    "$bin" --notifier restart || true
    return 0
  fi
  [[ "$1" == ask ]] || return 0
  printf '\n' >&2
  say "$(t "Сповіщення про оновлення" "Update notifications")"
  printf "$(t '  Раз на 12 годин OBS-Legacy у фоні перевірятиме, чи вийшли нові OBS, плагіни чи сама програма,\n' \
              '  Every 12 hours OBS-Legacy will check in the background whether a new OBS, plugins or the program itself are out,\n')" >&2
  printf "$(t '  а після оновлень системи — чи власному OBS не бракує бібліотек. Без sudo; вимкнути можна в меню.\n' \
              '  and after system upgrades — whether your own OBS is missing libraries. No sudo; you can turn it off in the menu.\n')" >&2
  # Відповідь запам'ятовується: тоді меню не вмикатиме сповіщення само (obs-legacy вмикає їх типово).
  mkdir -p "${XDG_STATE_HOME:-$HOME/.local/state}/obs-legacy" \
    && : > "${XDG_STATE_HOME:-$HOME/.local/state}/obs-legacy/notifier-decided" || true
  if confirm_yes "$(t "Увімкнути сповіщення?" "Turn on notifications?")"; then
    "$bin" --notifier enable || warn "$(t "Не вдалося ввімкнути сповіщення — спробуй пізніше: obs-legacy → Додатково → «Сповіщення про оновлення»." \
                                          "Could not turn on notifications — try later: obs-legacy → More → “Update notifications”.")"
  else
    printf "$(t '  Добре. Увімкнути пізніше: obs-legacy → Додатково → «Сповіщення про оновлення».\n' \
                '  OK. To turn them on later: obs-legacy → More → “Update notifications”.\n')" >&2
  fi
}

banner() {
  printf "$(t '\n%sOBS-Legacy%s — OBS Studio з NVENC для NVIDIA 580xx (Artix / Arch)\n' \
              '\n%sOBS-Legacy%s — OBS Studio with NVENC for NVIDIA 580xx (Artix / Arch)\n')" "$C_GRN$C_BLD" "$C_RST" >&2
  printf "$(t '%sСкрипт зроблений за допомогою Claude Code (Anthropic).%s\n' \
              '%sThis script was made with Claude Code (Anthropic).%s\n')" "$C_YEL$C_BLD" "$C_RST" >&2
  printf "$(t 'Натхнення: https://github.com/sl5net/build-obs-nvenc-legacy — дякуємо!\n\n' \
              'Inspired by: https://github.com/sl5net/build-obs-nvenc-legacy — thank you!\n\n')" >&2
}

# Чим ставити й прибирати пакети від root: sudo чи doas.
root_tool() {
  if have sudo; then printf 'sudo\n'; elif have doas; then printf 'doas\n'; else return 1; fi
}

# Без термінала відповісти pacman на «Продовжити? [Y/n]» нікому (так install.sh запускає перевірка
# в CI). Тоді пароль теж ніхто не введе, тож sudo/doas спрацює лише там, де його дозволено без пароля.
pacman_opts() {
  if [[ ! -t 0 ]]; then printf '%s\n' --noconfirm; fi
}

# Чи збігається сума SHA-256 файлу з його .sha256 з того самого релізу (перше поле першого рядка).
sha256_matches() {   # $1 = файл, $2 = файл із сумою
  local want got
  want=$(awk 'NR == 1 { print $1 }' "$2")
  got=$(sha256sum -- "$1" | awk '{ print $1 }')
  [[ "$want" =~ ^[0-9a-f]{64}$ && "$want" == "$got" ]]
}

ensure_rust() {
  local priv=''
  if have cargo; then return 0; fi
  if have rustup; then
    say "$(t "Є rustup, але без тулчейна — ставлю stable (rustup default stable)" \
             "rustup is there but without a toolchain — installing stable (rustup default stable)")"
    rustup default stable
    if have cargo; then return 0; fi
  fi
  warn "$(t "Для збирання TUI потрібен Rust (cargo)." "Rust (cargo) is needed to build the TUI.")"
  have pacman || die "$(t "pacman не знайдено. Встанови Rust вручну (https://rustup.rs) і повтори." \
                          "pacman was not found. Install Rust manually (https://rustup.rs) and try again.")"
  priv=$(root_tool) || die "$(t "Немає ні sudo, ні doas — встанови пакет rust вручну." \
                                "Neither sudo nor doas is available — install the rust package manually.")"
  confirm "$(t "Встановити пакет rust з репозиторію ($priv pacman -S --needed rust)?" \
               "Install the rust package from the repository ($priv pacman -S --needed rust)?")" \
    || die "$(t "Скасовано. Без Rust зібрати OBS-Legacy не вийде. Готову програму без Rust ставить ./install.sh --prebuilt." \
                "Cancelled. OBS-Legacy cannot be built without Rust. ./install.sh --prebuilt installs the ready-made program without Rust.")"
  "$priv" pacman -S --needed rust
  have cargo || die "$(t "Після встановлення cargo так і не знайшовся." \
                         "cargo still was not found after the installation.")"
}

# Проміжні файли cargo після встановлення не потрібні (бінарник уже скопійовано),
# а важать сотні мегабайтів. Наступне збирання піде з нуля — хвилину-дві.
clean_build_cache() {
  local size
  [[ -d "$TARGET_DIR" ]] || return 0
  size=$(du -sh -- "$TARGET_DIR" 2>/dev/null | cut -f1 || true)
  if (cd -- "$SRC_DIR" && cargo clean --quiet --target-dir "$TARGET_DIR"); then
    ok "$(t "Прибрано проміжні файли збирання Rust: $TARGET_DIR (${size:-?})" \
            "Removed the intermediate Rust build files: $TARGET_DIR (${size:-?})")"
  else
    warn "$(t "Не вдалося прибрати $TARGET_DIR — прибери вручну: cd '$SRC_DIR' && cargo clean" \
              "Could not remove $TARGET_DIR — remove it manually: cd '$SRC_DIR' && cargo clean")"
  fi
}

# З теки з вихідними кодами: програма збирається під цей процесор.
build_from_source() {
  # Явно, а не з CARGO_TARGET_DIR: звідси беремо бінарник і цю теку потім прибираємо.
  TARGET_DIR="$SRC_DIR/target"
  ensure_rust
  say "$(t "Збираю obs-legacy (cargo build --release --locked) — це займе хвилину-дві" \
           "Building obs-legacy (cargo build --release --locked) — this takes a minute or two")"
  # Як і OBS, програму збираємо під цей процесор (вона ставиться лише на нього).
  (cd -- "$SRC_DIR" && RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C target-cpu=native" \
     cargo build --release --locked --target-dir "$TARGET_DIR")

  say "$(t "Встановлюю" "Installing")"
  put "$TARGET_DIR/release/obs-legacy" "$BIN_DIR/obs-legacy" 755
  ok "$(t "Програма: $BIN_DIR/obs-legacy" "Program: $BIN_DIR/obs-legacy")"
  clean_build_cache
  put "$SRC_DIR/assets/obs-legacy.svg" "$ICON_DIR/obs-legacy.svg" 644
}

require_x86_64() {
  local arch
  arch=$(uname -m)
  [[ "$arch" == x86_64 ]] || die "$(t "Готова програма є лише для x86_64, а тут $arch. Збери її з вихідних кодів: git clone $REPO_URL.git && cd OBS-Legacy && ./install.sh" \
                                      "The ready-made program is for x86_64 only, and this is $arch. Build it from source: git clone $REPO_URL.git && cd OBS-Legacy && ./install.sh")"
}

# Завантажити файл релізу разом з його .sha256 у TMP_DIR і перевірити суму.
fetch_checked() {   # $1 = назва файлу, $2 = що це (для повідомлення)
  local url
  url=$(release_url "$1")
  say "$(t "Завантажую $2: $url" "Downloading $2: $url")"
  if ! fetch "$url" "$TMP_DIR/$1" || ! fetch "$url.sha256" "$TMP_DIR/$1.sha256"; then
    die "$(t "Не вдалося завантажити $url. Перевір інтернет і чи відкривається $REPO_URL/releases, і спробуй ще раз." \
             "Could not download $url. Check the internet connection and that $REPO_URL/releases opens, then try again.")"
  fi
  sha256_matches "$TMP_DIR/$1" "$TMP_DIR/$1.sha256" \
    || die "$(t "Контрольна сума не збігається: файл пошкодився під час завантаження. Спробуй ще раз." \
                "The checksum does not match: the file got damaged while downloading. Try again.")"
  ok "$(t "Сума SHA-256 збігається" "The SHA-256 checksum matches")"
}

# Тег релізу: закріплений у копії install.sh з релізу (package.yml), інакше найновіший — GitHub
# переадресовує releases/latest на сторінку його тега.
release_tag() {
  local url
  if [[ -n "$RELEASE_TAG" ]]; then printf '%s\n' "$RELEASE_TAG"; return 0; fi
  if [[ -n "${OBS_LEGACY_DOWNLOAD_URL:-}" ]] || ! have curl; then return 1; fi
  url=$(curl -fsSIL --retry 3 --connect-timeout 20 --speed-limit 1 --speed-time 120 \
          -o /dev/null -w '%{url_effective}' "$REPO_URL/releases/latest") || return 1
  url=${url##*/}
  [[ "$url" =~ ^v[0-9][0-9A-Za-z.]*$ ]] || return 1
  printf '%s\n' "$url"
}

# Пакет pacman obs-legacy з релізу — той самий бінарник плюс хук pacman, що оновлює пакети OBS-Legacy
# разом із системою (копія в домашній теці цього не вміє: pacman бере хуки лише з /usr і /etc).
# Ставиться локальним файлом: підписів .sig у релізах немає, тож `pacman -U <URL>` з SigLevel = Required
# відмовив би, а локальний файл pacman перевіряє за LocalFileSigLevel. Цілість — сума з .sha256.
install_package() {
  local priv tag file installed version
  local -a opts
  require_x86_64
  priv=$(root_tool) || die "$(t "Немає ні sudo, ні doas, тож пакет не поставити. Готову програму без пакета ставить: … | bash -s -- --no-package" \
                                "Neither sudo nor doas is available, so the package cannot be installed. The ready-made program without the package: … | bash -s -- --no-package")"
  tag=$(release_tag) || die "$(t "Не вдалося дізнатися найновіший реліз. Перевір інтернет і чи відкривається $REPO_URL/releases, і спробуй ще раз." \
                                 "Could not find out the latest release. Check the internet connection and that $REPO_URL/releases opens, then try again.")"
  file="obs-legacy-${tag#v}-1-x86_64.pkg.tar.zst"
  installed=$(pacman -Q obs-legacy 2>/dev/null | awk '{ print $2 }' || true)
  if [[ "$installed" == "${tag#v}-1" ]]; then
    ok "$(t "Пакет obs-legacy $installed уже встановлено" "The obs-legacy package $installed is already installed")"
    return 0
  fi
  TMP_DIR=$(mktemp -d)
  fetch_checked "$file" "$(t "пакет obs-legacy" "the obs-legacy package")"
  mapfile -t opts < <(pacman_opts)
  say "$(t "Ставлю пакет: $priv pacman -U $file (знадобиться пароль)" \
           "Installing the package: $priv pacman -U $file (the password is needed)")"
  "$priv" pacman -U "${opts[@]}" "$TMP_DIR/$file" \
    || die "$(t "pacman не поставив пакет, система не змінилась. Спробуй ще раз або постав програму без пакета: … | bash -s -- --no-package" \
                "pacman did not install the package, the system did not change. Try again or install the program without the package: … | bash -s -- --no-package")"
  rm -rf -- "$TMP_DIR"
  TMP_DIR=''
  version=$("$PKG_EXE" --version 2>/dev/null || true)
  ok "$(t "Програма: $PKG_EXE ($version)" "Program: $PKG_EXE ($version)")"
}

# Копію, яку install.sh колись поклав у ~/.local/bin, пакет переймає сам (obs-legacy.install від
# імені того, хто запустив sudo). Якщо pacman не дізнався користувача — те саме тут; повтор нешкідливий.
adopt_home_copy() {
  [[ -e "$BIN_DIR/obs-legacy" ]] || return 0
  "$PKG_EXE" --use-package \
    || warn "$(t "Стара копія $BIN_DIR/obs-legacy лишилась і перекриває пакет. Прибери її: $PKG_EXE --use-package" \
                 "The old copy $BIN_DIR/obs-legacy is still there and hides the package. Remove it: $PKG_EXE --use-package")"
}

# Готова програма з релізу — той самий бінарник, що в пакеті obs-legacy (зібраний у
# чистому контейнері Arch), лише без пакета. Сума SHA-256 — з того самого релізу.
install_prebuilt() {
  local version
  require_x86_64
  have pacman || warn "$(t "pacman не знайдено: OBS-Legacy збирає й ставить OBS лише на Artix і Arch." \
                           "pacman was not found: OBS-Legacy builds and installs OBS on Artix and Arch only.")"
  TMP_DIR=$(mktemp -d)
  fetch_checked "$ASSET" "$(t "готову програму" "the ready-made program")"
  chmod 755 -- "$TMP_DIR/$ASSET"
  version=$("$TMP_DIR/$ASSET" --version 2>/dev/null) \
    || die "$(t "Завантажена програма не запускається тут (застаріла glibc?). Онови систему (sudo pacman -Syu) або збери програму з вихідних кодів: git clone $REPO_URL.git && cd OBS-Legacy && ./install.sh" \
                "The downloaded program does not run here (an old glibc?). Update the system (sudo pacman -Syu) or build the program from source: git clone $REPO_URL.git && cd OBS-Legacy && ./install.sh")"

  say "$(t "Встановлюю" "Installing")"
  put "$TMP_DIR/$ASSET" "$BIN_DIR/obs-legacy" 755
  ok "$(t "Програма: $BIN_DIR/obs-legacy ($version)" "Program: $BIN_DIR/obs-legacy ($version)")"
  if fetch "$(icon_url)" "$TMP_DIR/obs-legacy.svg" && grep -qs '<svg' "$TMP_DIR/obs-legacy.svg"; then
    put "$TMP_DIR/obs-legacy.svg" "$ICON_DIR/obs-legacy.svg" 644
  else
    warn "$(t "Не вдалося завантажити іконку — ярлик буде без неї." "Could not download the icon — the shortcut will have none.")"
  fi
  rm -rf -- "$TMP_DIR"
  TMP_DIR=''
}

install_desktop_shortcut() {   # $1 = програма
  local dir file tmp
  dir=$(desktop_dir)
  if [[ ! -d "$dir" ]]; then
    warn "$(t "Теки робочого столу $dir немає — ярлик не додаю." \
              "There is no desktop folder $dir — not adding a shortcut.")"
    return 0
  fi
  file="$dir/$DESKTOP_ID"
  if [[ -e "$file" ]] && ! grep -qs -- "$MARKER" "$file"; then
    warn "$(t "$file уже існує і створений не цим установником — не чіпаю." \
              "$file already exists and was not created by this installer — leaving it alone.")"
    return 0
  fi
  tmp=$(mktemp)
  write_desktop_entry "$tmp" "$1"
  put "$tmp" "$file" 755
  rm -f -- "$tmp"
  # GNOME / Nautilus і Xfce вимагають позначити ярлик як довірений.
  if have gio; then
    gio set "$file" metadata::trusted true 2>/dev/null || true
    if [[ "${XDG_CURRENT_DESKTOP:-}" == *XFCE* ]]; then
      gio set -t string "$file" metadata::xfce-exe-checksum "$(sha256sum -- "$file" | awk '{print $1}')" 2>/dev/null || true
    fi
  fi
  ok "$(t "Ярлик на робочому столі: $file" "Desktop shortcut: $file")"
}

# Ярлик у меню програм для копії в ~/.local/bin (у пакета він свій, у /usr/share/applications) і
# позначка, звідки програма оновлюватиме себе з меню (script::self_source): тека з вихідними кодами чи реліз.
install_home_menu_entry() {
  local tmp
  tmp=$(mktemp)
  write_desktop_entry "$tmp" "$BIN_DIR/obs-legacy"
  put "$tmp" "$APPS_DIR/$DESKTOP_ID" 644
  rm -f -- "$tmp"
  if have update-desktop-database; then update-desktop-database -q "$APPS_DIR" 2>/dev/null || true; fi
  if have gtk-update-icon-cache && [[ -f "$DATA_HOME/icons/hicolor/index.theme" ]]; then
    gtk-update-icon-cache -q -t "$DATA_HOME/icons/hicolor" 2>/dev/null || true
  fi
  ok "$(t "Ярлик у меню програм: OBS-Legacy ($APPS_DIR/$DESKTOP_ID)" \
          "Applications menu shortcut: OBS-Legacy ($APPS_DIR/$DESKTOP_ID)")"
  mkdir -p -- "$STATE_DIR"
  if [[ -n "$SRC_DIR" ]]; then
    printf '%s\n' "$SRC_DIR" > "$STATE_DIR/source-dir"
    rm -f -- "$STATE_DIR/release-install"
  else
    printf '%s\n' "${RELEASE_TAG:-latest}" > "$STATE_DIR/release-install"
    rm -f -- "$STATE_DIR/source-dir"
  fi
}

do_install() {   # $1 = yes|no|ask|update — ярлик на робочому столі
  local want_desktop=$1 program had_shortcut=0
  (( EUID != 0 )) || die "$(t "Не запускай від root: TUI ставиться для звичайного користувача." \
                              "Do not run as root: the TUI is installed for a normal user.")"
  banner
  # Ярлик на столі від install.sh: перехід на пакет прибирає його разом зі старою копією, а тут він
  # з'явиться знову — уже для програми з пакета.
  if grep -qs -- "$MARKER" "$(desktop_dir)/$DESKTOP_ID"; then had_shortcut=1; fi
  if [[ -z "$SRC_DIR" ]] && (( USE_PACKAGE )) && have pacman; then
    install_package
    adopt_home_copy
    program=$PKG_EXE
  else
    # Програма вже стоїть готовим пакетом: копія в ~/.local/bin перекриє її (PATH, ярлик).
    if have pacman && pacman -Qq obs-legacy >/dev/null 2>&1; then
      warn "$(t "Пакет obs-legacy уже встановлено ($PKG_EXE), і копія в $BIN_DIR його перекриє. Залиш щось одне: sudo pacman -R obs-legacy або потім install.sh --uninstall." \
                "The obs-legacy package is already installed ($PKG_EXE), and the copy in $BIN_DIR will override it. Keep only one: sudo pacman -R obs-legacy or later install.sh --uninstall.")"
    fi
    if [[ -n "$SRC_DIR" ]]; then build_from_source; else install_prebuilt; fi
    install_home_menu_entry
    program="$BIN_DIR/obs-legacy"
  fi

  case "$want_desktop" in
    yes) install_desktop_shortcut "$program" ;;
    ask)
      if (( had_shortcut )) || confirm "$(t "Додати ярлик OBS-Legacy на робочий стіл?" "Add an OBS-Legacy shortcut to the desktop?")"; then
        install_desktop_shortcut "$program"
      fi
      ;;
    update)
      if (( had_shortcut )); then install_desktop_shortcut "$program"; fi
      ;;
  esac

  if [[ "$want_desktop" == update ]]; then setup_notifier update "$program"; else setup_notifier ask "$program"; fi

  if [[ "$program" == "$BIN_DIR/obs-legacy" ]]; then
    case ":$PATH:" in
      *":$BIN_DIR:"*) ;;
      *) warn "$(t "$BIN_DIR немає в PATH: команда obs-legacy в терміналі не знайдеться (ярлик у меню працює). Додай у ~/.bashrc або ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\"" \
                   "$BIN_DIR is not in PATH: the obs-legacy command will not be found in a terminal (the menu shortcut works). Add to ~/.bashrc or ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\"")" ;;
    esac
  fi

  printf "$(t '\n%sГОТОВО ✓%s\n' '\n%sDONE ✓%s\n')" "$C_GRN$C_BLD" "$C_RST" >&2
  printf "$(t '  Запуск: меню програм → «OBS-Legacy» (або команда obs-legacy). Клацання відкриє термінал з меню.\n' \
              '  Launch: applications menu → “OBS-Legacy” (or the obs-legacy command). Clicking it opens a terminal with the menu.\n')" >&2
  if [[ "$program" == "$PKG_EXE" ]]; then
    printf "$(t '  Оновити OBS-Legacy: у меню obs-legacy → Додатково → «Оновити OBS-Legacy» (новий пакет з релізу).\n' \
                '  Update OBS-Legacy: in the obs-legacy menu → More → “Update OBS-Legacy” (the new package from the release).\n')" >&2
  elif [[ -n "$SRC_DIR" ]]; then
    printf "$(t '  Оновити OBS-Legacy: у меню obs-legacy → «Оновити OBS-Legacy» (або git pull && ./install.sh).\n' \
                '  Update OBS-Legacy: in the obs-legacy menu → “Update OBS-Legacy” (or git pull && ./install.sh).\n')" >&2
  else
    printf "$(t '  Оновити OBS-Legacy: ще раз та сама команда (curl … | bash).\n' \
                '  Update OBS-Legacy: run the same command again (curl … | bash).\n')" >&2
  fi
  if [[ "$want_desktop" != update ]] && confirm_yes "$(t "Відкрити OBS-Legacy зараз?" "Open OBS-Legacy now?")"; then
    exec "$program"
  fi
}

do_uninstall() {
  local f installed program='' priv
  local -a opts
  banner
  say "$(t "Прибираю програму OBS-Legacy (пакети OBS, плагінів і ffmpeg це НЕ видаляє)" \
           "Removing the OBS-Legacy program (this does NOT remove the OBS, plugin and ffmpeg packages)")"
  installed=$(pacman -Qq 2>/dev/null | grep -E -- '-legacy-580xx$' | tr '\n' ' ' || true)
  if [[ -n "$installed" || -d /opt/obs-studio-nvenc ]]; then
    warn "$(t "Лишаються встановленими: ${installed:-стара збірка в /opt/obs-studio-nvenc}. Щоб прибрати і їх, спершу запусти obs-legacy → «Видалити OBS-Legacy»." \
              "Still installed: ${installed:-the old build in /opt/obs-studio-nvenc}. To remove them too, first run obs-legacy → “Remove OBS-Legacy”.")"
    confirm "$(t "Прибрати лише програму?" "Remove only the program?")" || exit 1
  fi
  if [[ -x "$BIN_DIR/obs-legacy" ]]; then program="$BIN_DIR/obs-legacy"; elif [[ -x "$PKG_EXE" ]]; then program=$PKG_EXE; fi
  if [[ -n "$program" ]]; then
    "$program" --notifier disable || warn "$(t "Сповіщення не вдалося вимкнути: obs-legacy --notifier status підкаже, що лишилось." \
                                               "Could not turn off notifications: obs-legacy --notifier status shows what is left.")"
  fi
  rm -rf -- "${XDG_STATE_HOME:-$HOME/.local/state}/obs-legacy"
  rm -f -- "$BIN_DIR/obs-legacy" "$ICON_DIR/obs-legacy.svg" "$STATE_DIR/source-dir" "$STATE_DIR/release-install"
  for f in "$APPS_DIR/$DESKTOP_ID" "$(desktop_dir)/$DESKTOP_ID"; do
    if grep -qs -- "$MARKER" "$f"; then rm -f -- "$f"; fi
  done
  if have update-desktop-database; then update-desktop-database -q "$APPS_DIR" 2>/dev/null || true; fi
  # Розпакований скрипт збирання потрібен, доки встановлені пакети OBS-Legacy
  # чи стара збірка в /opt: на нього посилаються підказки pacman-хука й обгортки.
  if [[ -z "$installed" && ! -d /opt/obs-studio-nvenc ]]; then
    rm -f -- "$STATE_DIR/build-obs-nvenc-legacy-artix.sh"
    rmdir -- "$STATE_DIR" 2>/dev/null || true
  fi
  if pacman -Qq obs-legacy >/dev/null 2>&1; then
    priv=$(root_tool) || die "$(t "Немає ні sudo, ні doas: прибери пакет від root: pacman -R obs-legacy" \
                                  "Neither sudo nor doas is available: remove the package as root: pacman -R obs-legacy")"
    mapfile -t opts < <(pacman_opts)
    say "$(t "Прибираю пакет: $priv pacman -R obs-legacy" "Removing the package: $priv pacman -R obs-legacy")"
    "$priv" pacman -R "${opts[@]}" obs-legacy \
      || die "$(t "pacman не прибрав пакет obs-legacy. Спробуй ще раз: sudo pacman -R obs-legacy" \
                  "pacman did not remove the obs-legacy package. Try again: sudo pacman -R obs-legacy")"
  fi
  ok "$(t "Програму прибрано. Налаштування лишились у ${XDG_CONFIG_HOME:-$HOME/.config}/obs-legacy/ — видали вручну, якщо не потрібні." \
          "The program is removed. The settings are left in ${XDG_CONFIG_HOME:-$HOME/.config}/obs-legacy/ — delete them manually if not needed.")"
}

usage() {
  if ((LANG_EN)); then
    cat <<'EOF'
install.sh — installs the OBS-Legacy program for the current user
https://github.com/YellowHearth1/OBS-Legacy

Made with Claude Code (Anthropic's AI assistant), commissioned and directed by the
repository owner. Inspired by: https://github.com/sl5net/build-obs-nvenc-legacy

What it does:
  * without a source folder (curl … | bash) on Arch and Artix it installs the obs-legacy
    pacman package from a GitHub release: it checks its SHA-256 and installs it as a local
    file (sudo pacman -U). With the package, OBS-Legacy updates its packages together with
    the system. A copy that install.sh once put into ~/.local/bin is taken over by the
    package;
  * without pacman, or with --no-package, it puts the same ready-made program into
    ~/.local/bin (no sudo, no Rust, no build);
  * in a source folder (git clone) it builds obs-legacy for this processor
    (cargo build --release --locked) into ~/.local/bin and, after installing, removes the
    intermediate build files (target/, hundreds of megabytes);
  * adds an "OBS-Legacy" shortcut to the applications menu and an icon (the package has
    its own), and optionally a desktop shortcut. Clicking the shortcut opens a terminal
    window with the OBS-Legacy menu right away;
  * optionally turns on notifications about new versions of OBS, plugins and OBS-Legacy
    itself: a systemd, dinit, OpenRC, runit or s6 user service, or session autostart
    (obs-legacy --notifier, no sudo).
It installs only the obs-legacy package into the system and (with your consent) the rust
package for building.

Usage:
  curl -fsSL https://github.com/YellowHearth1/OBS-Legacy/releases/latest/download/install.sh | bash
  ./install.sh              install or update (asks about a desktop shortcut)
  ./install.sh --desktop    the same + a desktop shortcut without asking
  ./install.sh --no-desktop no desktop shortcut
  ./install.sh --prebuilt   the ready-made program from a release, even in a source folder
  ./install.sh --no-package the ready-made program into ~/.local/bin, no pacman package, no sudo
  ./install.sh --update     no questions (this is how obs-legacy itself runs it)
  ./install.sh --uninstall  remove the program and notifications (the built OBS stays)
Options together with curl: … | bash -s -- --uninstall
EOF
  else
    cat <<'EOF'
install.sh — встановлює програму OBS-Legacy для поточного користувача
https://github.com/YellowHearth1/OBS-Legacy

Створено за допомогою Claude Code (ШІ-асистент Anthropic) на замовлення і під
керівництвом автора репозиторію. Натхнення: https://github.com/sl5net/build-obs-nvenc-legacy

Що робить:
  * без теки з вихідними кодами (curl … | bash) на Arch і Artix ставить пакет pacman
    obs-legacy з релізу на GitHub: перевіряє його суму SHA-256 і ставить локальним файлом
    (sudo pacman -U). З пакетом OBS-Legacy оновлює свої пакети разом із системою. Копію,
    яку install.sh колись поклав у ~/.local/bin, пакет переймає сам;
  * без pacman або з --no-package кладе ту саму готову програму в ~/.local/bin (без sudo,
    без Rust і без збирання);
  * у теці з вихідними кодами (git clone) збирає obs-legacy під цей процесор
    (cargo build --release --locked) у ~/.local/bin, а після встановлення прибирає
    проміжні файли збирання (target/, сотні мегабайтів);
  * додає ярлик «OBS-Legacy» у меню програм та іконку (у пакеті вони свої), за бажанням —
    ярлик на робочий стіл. Клацання по ярлику одразу відкриває вікно термінала з меню
    OBS-Legacy;
  * за бажанням вмикає сповіщення про нові версії OBS, плагінів і самого OBS-Legacy:
    службу користувача systemd, dinit, OpenRC, runit чи s6 або автозапуск сесії
    (obs-legacy --notifier, без sudo).
У систему ставить лише пакет obs-legacy і (з твоєї згоди) пакет rust для збирання.

Використання:
  curl -fsSL https://github.com/YellowHearth1/OBS-Legacy/releases/latest/download/install.sh | bash
  ./install.sh              встановити або оновити (запитає про ярлик на столі)
  ./install.sh --desktop    те саме + ярлик на робочому столі без запитання
  ./install.sh --no-desktop без ярлика на робочому столі
  ./install.sh --prebuilt   готова програма з релізу, навіть у теці з вихідними кодами
  ./install.sh --no-package готова програма в ~/.local/bin, без пакета pacman і sudo
  ./install.sh --update     без запитань (так його викликає сам obs-legacy)
  ./install.sh --uninstall  прибрати програму й сповіщення (зібраний OBS не чіпає)
Параметри разом із curl: … | bash -s -- --uninstall
EOF
  fi
}

main() {
  local desktop=ask arg
  if [[ -t 0 ]]; then INTERACTIVE=1; fi
  for arg in "$@"; do
    case "$arg" in
      --desktop)    desktop=yes ;;
      --no-desktop) desktop=no ;;
      --prebuilt)   SRC_DIR='' ;;
      --no-package) SRC_DIR=''; USE_PACKAGE=0 ;;
      --update)     desktop=update; INTERACTIVE=0 ;;
      --uninstall)  do_uninstall; return ;;
      -h|--help)    usage; return ;;
      *)            die "$(t "Невідомий параметр: $arg (див. install.sh --help)" \
                             "Unknown option: $arg (see install.sh --help)")" ;;
    esac
  done
  do_install "$desktop"
}

# «curl … | bash»: stdin — це сам скрипт, тож відповіді й меню наприкінці — з термінала.
if [[ ! -t 0 ]] && { true </dev/tty; } 2>/dev/null; then
  main "$@" </dev/tty
else
  main "$@"
fi
