#!/usr/bin/env bash
# shellcheck disable=SC1111  # типографські лапки “…” в англійських повідомленнях — навмисні
# =============================================================================
#  install.sh — встановлює TUI OBS-Legacy для поточного користувача
#  https://github.com/YellowHearth1/OBS-Legacy
# -----------------------------------------------------------------------------
#  Створено за допомогою Claude Code (ШІ-асистент Anthropic) на замовлення і
#  під керівництвом автора репозиторію. Натхнення:
#  https://github.com/sl5net/build-obs-nvenc-legacy
#
#  Що робить:
#    * у теці з вихідними кодами (git clone) збирає obs-legacy під цей процесор
#      (cargo build --release --locked), а після встановлення прибирає проміжні
#      файли збирання (target/, сотні мегабайтів);
#    * без неї (curl … | bash) завантажує з релізу на GitHub готову програму й
#      перевіряє її суму SHA-256 — без Rust і без збирання;
#    * кладе obs-legacy в ~/.local/bin, додає ярлик «OBS-Legacy» у меню програм
#      та іконку, за бажанням — ярлик на робочий стіл. Клацання по ярлику одразу
#      відкриває вікно термінала з меню OBS-Legacy;
#    * за бажанням вмикає сповіщення про нові версії OBS, плагінів і самого
#      OBS-Legacy: службу користувача systemd, dinit, OpenRC, runit чи s6 або
#      автозапуск сесії (obs-legacy --notifier, без sudo).
#  У систему нічого не ставить, крім (лише з твоєї згоди) пакета rust.
#
#  Використання:
#    curl -fsSL https://github.com/YellowHearth1/OBS-Legacy/releases/latest/download/install.sh | bash
#    ./install.sh              встановити або оновити (запитає про ярлик на столі)
#    ./install.sh --desktop    те саме + ярлик на робочому столі без запитання
#    ./install.sh --no-desktop без ярлика на робочому столі
#    ./install.sh --prebuilt   готова програма з релізу, навіть у теці з вихідними кодами
#    ./install.sh --update     без запитань (так його викликає сам obs-legacy)
#    ./install.sh --uninstall  прибрати TUI і сповіщення (зібраний OBS не чіпає)
#  Параметри разом із curl: … | bash -s -- --uninstall
# =============================================================================
set -Eeuo pipefail

REPO_URL="https://github.com/YellowHearth1/OBS-Legacy"
# Реліз, з якого брати готову програму; порожньо — найновіший. У копію install.sh,
# яку робочий процес Package прикріплює до релізу, підставляється тег цього релізу.
RELEASE_TAG="${OBS_LEGACY_TAG:-}"
ASSET="obs-legacy-x86_64"

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
      v=$(sed -n 's/^[[:space:]]*UI_LANG=//p' "$file" 2>/dev/null | tail -n1 | tr -d "\"' " || true)
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

# Завантажити файл: curl (є всюди, де є pacman) або wget.
fetch() {   # $1 = URL, $2 = куди
  if have curl; then
    curl -fsSL --retry 3 --connect-timeout 20 -o "$2" "$1"
  elif have wget; then
    wget -q -O "$2" "$1"
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

write_desktop_entry() {   # $1 = файл
  local exec_path="$BIN_DIR/obs-legacy"
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
setup_notifier() {   # $1 = update|ask
  local bin="$BIN_DIR/obs-legacy"
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
  if have sudo; then priv=sudo; elif have doas; then priv=doas; else die "$(t "Немає ні sudo, ні doas — встанови пакет rust вручну." \
                                                                              "Neither sudo nor doas is available — install the rust package manually.")"; fi
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

# Готова програма з релізу — той самий бінарник, що в пакеті obs-legacy (зібраний у
# чистому контейнері Arch), лише без пакета. Сума SHA-256 — з того самого релізу.
install_prebuilt() {
  local arch url want got version
  arch=$(uname -m)
  [[ "$arch" == x86_64 ]] || die "$(t "Готова програма є лише для x86_64, а тут $arch. Збери її з вихідних кодів: git clone $REPO_URL.git && cd OBS-Legacy && ./install.sh" \
                                      "The ready-made program is for x86_64 only, and this is $arch. Build it from source: git clone $REPO_URL.git && cd OBS-Legacy && ./install.sh")"
  have pacman || warn "$(t "pacman не знайдено: OBS-Legacy збирає й ставить OBS лише на Artix і Arch." \
                           "pacman was not found: OBS-Legacy builds and installs OBS on Artix and Arch only.")"
  TMP_DIR=$(mktemp -d)
  url=$(release_url "$ASSET")
  say "$(t "Завантажую готову програму: $url" "Downloading the ready-made program: $url")"
  if ! fetch "$url" "$TMP_DIR/$ASSET" || ! fetch "$url.sha256" "$TMP_DIR/$ASSET.sha256"; then
    die "$(t "Не вдалося завантажити $url. Перевір інтернет і чи відкривається $REPO_URL/releases, і спробуй ще раз." \
             "Could not download $url. Check the internet connection and that $REPO_URL/releases opens, then try again.")"
  fi
  want=$(awk 'NR == 1 { print $1 }' "$TMP_DIR/$ASSET.sha256")
  got=$(sha256sum -- "$TMP_DIR/$ASSET" | awk '{ print $1 }')
  [[ "$want" =~ ^[0-9a-f]{64}$ && "$want" == "$got" ]] \
    || die "$(t "Контрольна сума не збігається: файл пошкодився під час завантаження. Спробуй ще раз." \
                "The checksum does not match: the file got damaged while downloading. Try again.")"
  ok "$(t "Сума SHA-256 збігається" "The SHA-256 checksum matches")"
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

install_desktop_shortcut() {
  local dir file
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
  put "$APPS_DIR/$DESKTOP_ID" "$file" 755
  # GNOME / Nautilus і Xfce вимагають позначити ярлик як довірений.
  if have gio; then
    gio set "$file" metadata::trusted true 2>/dev/null || true
    if [[ "${XDG_CURRENT_DESKTOP:-}" == *XFCE* ]]; then
      gio set -t string "$file" metadata::xfce-exe-checksum "$(sha256sum -- "$file" | awk '{print $1}')" 2>/dev/null || true
    fi
  fi
  ok "$(t "Ярлик на робочому столі: $file" "Desktop shortcut: $file")"
}

do_install() {   # $1 = yes|no|ask|update — ярлик на робочому столі
  local want_desktop=$1 tmp desktop_file
  (( EUID != 0 )) || die "$(t "Не запускай від root: TUI ставиться для звичайного користувача." \
                              "Do not run as root: the TUI is installed for a normal user.")"
  banner
  # Програма вже стоїть готовим пакетом: копія в ~/.local/bin перекриє її (PATH, ярлик).
  if have pacman && pacman -Qq obs-legacy >/dev/null 2>&1; then
    warn "$(t "Пакет obs-legacy уже встановлено (/usr/bin/obs-legacy), і копія в $BIN_DIR його перекриє. Залиш щось одне: sudo pacman -R obs-legacy або потім install.sh --uninstall." \
              "The obs-legacy package is already installed (/usr/bin/obs-legacy), and the copy in $BIN_DIR will override it. Keep only one: sudo pacman -R obs-legacy or later install.sh --uninstall.")"
  fi
  if [[ -n "$SRC_DIR" ]]; then build_from_source; else install_prebuilt; fi

  tmp=$(mktemp)
  write_desktop_entry "$tmp"
  put "$tmp" "$APPS_DIR/$DESKTOP_ID" 644
  rm -f -- "$tmp"
  if have update-desktop-database; then update-desktop-database -q "$APPS_DIR" 2>/dev/null || true; fi
  if have gtk-update-icon-cache && [[ -f "$DATA_HOME/icons/hicolor/index.theme" ]]; then
    gtk-update-icon-cache -q -t "$DATA_HOME/icons/hicolor" 2>/dev/null || true
  fi
  ok "$(t "Ярлик у меню програм: OBS-Legacy ($APPS_DIR/$DESKTOP_ID)" \
          "Applications menu shortcut: OBS-Legacy ($APPS_DIR/$DESKTOP_ID)")"
  # Звідки програма оновлюватиме себе з меню (script::self_source): тека з вихідними кодами чи реліз.
  mkdir -p -- "$STATE_DIR"
  if [[ -n "$SRC_DIR" ]]; then
    printf '%s\n' "$SRC_DIR" > "$STATE_DIR/source-dir"
    rm -f -- "$STATE_DIR/release-install"
  else
    printf '%s\n' "${RELEASE_TAG:-latest}" > "$STATE_DIR/release-install"
    rm -f -- "$STATE_DIR/source-dir"
  fi

  desktop_file="$(desktop_dir)/$DESKTOP_ID"
  case "$want_desktop" in
    yes) install_desktop_shortcut ;;
    ask)
      if grep -qs -- "$MARKER" "$desktop_file"; then
        install_desktop_shortcut
      elif confirm "$(t "Додати ярлик OBS-Legacy на робочий стіл?" "Add an OBS-Legacy shortcut to the desktop?")"; then
        install_desktop_shortcut
      fi
      ;;
    update)
      if grep -qs -- "$MARKER" "$desktop_file"; then install_desktop_shortcut; fi
      ;;
  esac

  if [[ "$want_desktop" == update ]]; then setup_notifier update; else setup_notifier ask; fi

  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$(t "$BIN_DIR немає в PATH: команда obs-legacy в терміналі не знайдеться (ярлик у меню працює). Додай у ~/.bashrc або ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\"" \
                 "$BIN_DIR is not in PATH: the obs-legacy command will not be found in a terminal (the menu shortcut works). Add to ~/.bashrc or ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\"")" ;;
  esac

  printf "$(t '\n%sГОТОВО ✓%s\n' '\n%sDONE ✓%s\n')" "$C_GRN$C_BLD" "$C_RST" >&2
  printf "$(t '  Запуск: меню програм → «OBS-Legacy» (або команда obs-legacy). Клацання відкриє термінал з меню.\n' \
              '  Launch: applications menu → “OBS-Legacy” (or the obs-legacy command). Clicking it opens a terminal with the menu.\n')" >&2
  if [[ -n "$SRC_DIR" ]]; then
    printf "$(t '  Оновити OBS-Legacy: у меню obs-legacy → «Оновити OBS-Legacy» (або git pull && ./install.sh).\n' \
                '  Update OBS-Legacy: in the obs-legacy menu → “Update OBS-Legacy” (or git pull && ./install.sh).\n')" >&2
  else
    printf "$(t '  Оновити OBS-Legacy: ще раз та сама команда (curl … | bash).\n' \
                '  Update OBS-Legacy: run the same command again (curl … | bash).\n')" >&2
  fi
  if [[ "$want_desktop" != update ]] && confirm_yes "$(t "Відкрити OBS-Legacy зараз?" "Open OBS-Legacy now?")"; then
    exec "$BIN_DIR/obs-legacy"
  fi
}

do_uninstall() {
  local f installed
  banner
  say "$(t "Прибираю TUI OBS-Legacy (пакети OBS-Legacy це НЕ видаляє)" \
           "Removing the OBS-Legacy TUI (this does NOT remove the OBS-Legacy packages)")"
  installed=$(pacman -Qq 2>/dev/null | grep -E -- '-legacy-580xx$' | tr '\n' ' ' || true)
  if [[ -n "$installed" || -d /opt/obs-studio-nvenc ]]; then
    warn "$(t "Лишаються встановленими: ${installed:-стара збірка в /opt/obs-studio-nvenc}. Щоб прибрати і їх, спершу запусти obs-legacy → «Видалити OBS-Legacy»." \
              "Still installed: ${installed:-the old build in /opt/obs-studio-nvenc}. To remove them too, first run obs-legacy → “Remove OBS-Legacy”.")"
    confirm "$(t "Прибрати лише TUI?" "Remove only the TUI?")" || exit 1
  fi
  if [[ -x "$BIN_DIR/obs-legacy" ]]; then
    "$BIN_DIR/obs-legacy" --notifier disable || warn "$(t "Сповіщення не вдалося вимкнути: obs-legacy --notifier status підкаже, що лишилось." \
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
  ok "$(t "TUI прибрано. Налаштування лишились у ${XDG_CONFIG_HOME:-$HOME/.config}/obs-legacy/ — видали вручну, якщо не потрібні." \
          "TUI removed. The settings are left in ${XDG_CONFIG_HOME:-$HOME/.config}/obs-legacy/ — delete them manually if not needed.")"
}

usage() {
  if ((LANG_EN)); then
    cat <<'EOF'
install.sh — installs the OBS-Legacy TUI for the current user
https://github.com/YellowHearth1/OBS-Legacy

Made with Claude Code (Anthropic's AI assistant), commissioned and directed by the
repository owner. Inspired by: https://github.com/sl5net/build-obs-nvenc-legacy

What it does:
  * in a source folder (git clone) it builds obs-legacy for this processor
    (cargo build --release --locked) and, after installing, removes the intermediate
    build files (target/, hundreds of megabytes);
  * without one (curl … | bash) it downloads the ready-made program from a GitHub
    release and checks its SHA-256 — no Rust, no build;
  * puts obs-legacy into ~/.local/bin, adds an "OBS-Legacy" shortcut to the applications
    menu and an icon, and optionally a desktop shortcut. Clicking the shortcut opens
    a terminal window with the OBS-Legacy menu right away;
  * optionally turns on notifications about new versions of OBS, plugins and OBS-Legacy
    itself: a systemd, dinit, OpenRC, runit or s6 user service, or session autostart
    (obs-legacy --notifier, no sudo).
It installs nothing into the system except (only with your consent) the rust package.

Usage:
  curl -fsSL https://github.com/YellowHearth1/OBS-Legacy/releases/latest/download/install.sh | bash
  ./install.sh              install or update (asks about a desktop shortcut)
  ./install.sh --desktop    the same + a desktop shortcut without asking
  ./install.sh --no-desktop no desktop shortcut
  ./install.sh --prebuilt   the ready-made program from a release, even in a source folder
  ./install.sh --update     no questions (this is how obs-legacy itself runs it)
  ./install.sh --uninstall  remove the TUI and notifications (the built OBS stays)
Options together with curl: … | bash -s -- --uninstall
EOF
  else
    cat <<'EOF'
install.sh — встановлює TUI OBS-Legacy для поточного користувача
https://github.com/YellowHearth1/OBS-Legacy

Створено за допомогою Claude Code (ШІ-асистент Anthropic) на замовлення і під
керівництвом автора репозиторію. Натхнення: https://github.com/sl5net/build-obs-nvenc-legacy

Що робить:
  * у теці з вихідними кодами (git clone) збирає obs-legacy під цей процесор
    (cargo build --release --locked), а після встановлення прибирає проміжні файли
    збирання (target/, сотні мегабайтів);
  * без неї (curl … | bash) завантажує з релізу на GitHub готову програму й перевіряє
    її суму SHA-256 — без Rust і без збирання;
  * кладе obs-legacy в ~/.local/bin, додає ярлик «OBS-Legacy» у меню програм та іконку,
    за бажанням — ярлик на робочий стіл. Клацання по ярлику одразу відкриває вікно
    термінала з меню OBS-Legacy;
  * за бажанням вмикає сповіщення про нові версії OBS, плагінів і самого OBS-Legacy:
    службу користувача systemd, dinit, OpenRC, runit чи s6 або автозапуск сесії
    (obs-legacy --notifier, без sudo).
У систему нічого не ставить, крім (лише з твоєї згоди) пакета rust.

Використання:
  curl -fsSL https://github.com/YellowHearth1/OBS-Legacy/releases/latest/download/install.sh | bash
  ./install.sh              встановити або оновити (запитає про ярлик на столі)
  ./install.sh --desktop    те саме + ярлик на робочому столі без запитання
  ./install.sh --no-desktop без ярлика на робочому столі
  ./install.sh --prebuilt   готова програма з релізу, навіть у теці з вихідними кодами
  ./install.sh --update     без запитань (так його викликає сам obs-legacy)
  ./install.sh --uninstall  прибрати TUI і сповіщення (зібраний OBS не чіпає)
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
