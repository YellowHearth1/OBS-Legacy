<p align="center">
  <a href="https://standforukraine.com/"><img src="https://yellowhearth1.neocities.org/img/support-ukraine-full.webp" alt="Підтримай Україну" width="520"></a>
</p>

<p align="center">
  <b>Україна захищається від повномасштабного вторгнення Росії.</b><br>
  Підтримай Збройні сили України й гуманітарну допомогу через перевірені фонди:<br>
  <a href="https://standforukraine.com/"><b>standforukraine.com</b></a>
</p>

---

<p align="center">
  <img src="assets/banner.png" alt="OBS-Legacy — OBS Studio з робочим NVENC на драйверах NVIDIA 580xx" width="820">
</p>

<p align="center">
  <a href="README.md">English</a> · <b>Українська</b>
</p>

<p align="center">
  <b>OBS Studio з робочим NVENC для відеокарт NVIDIA на драйверах 580xx (GTX 900, GTX 1000, Titan V).</b><br>
  Artix і Arch Linux · зрозуміле меню в терміналі · перевірка, встановлення, оновлення й відкат
</p>

<p align="center">
  <a href="https://github.com/YellowHearth1/OBS-Legacy/actions/workflows/ci.yml"><img src="https://github.com/YellowHearth1/OBS-Legacy/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="Ліцензія Apache-2.0"></a>
  <img src="https://img.shields.io/badge/Artix%20%7C%20Arch-Linux-1793d1" alt="Artix | Arch Linux">
  <img src="https://img.shields.io/badge/NVIDIA-580xx-76b900" alt="NVIDIA 580xx">
  <img src="https://img.shields.io/badge/version-0.0.2-555" alt="Версія 0.0.2">
</p>

> [!WARNING]
> **Скрипт зроблений за допомогою Claude Code.**
>
> Увесь проєкт — програму `obs-legacy`, скрипт збирання та установник — створено за допомогою
> [Claude Code](https://claude.com/claude-code), ШІ-асистента для програмування від Anthropic, на
> замовлення й під керівництвом автора репозиторію [YellowHearth1](https://github.com/YellowHearth1).
>
> - Код написав штучний інтелект; незалежного аудиту безпеки він не проходив.
> - Програма збирає OBS Studio, плагіни й ffmpeg з вихідних кодів, пакує їх у пакети pacman і ставить
>   через `pacman` (лише після твого підтвердження) за допомогою `sudo`/`doas`.
> - Почни з **«Перевірити систему»**: вона нічого не змінює.
> - Використовуєш на власний ризик, без жодних гарантій (див. [LICENSE](LICENSE)).
>
> Те саме попередження програма показує на першому екрані при кожному запуску.

<details>
<summary>Так виглядає перший екран програми</summary>
<p align="center">
  <img src="docs/screenshots/intro.png" alt="Перший екран: попередження «Скрипт зроблений за допомогою Claude Code» і подяка sl5net" width="720">
</p>
</details>

> [!NOTE]
> **Натхнення.** OBS-Legacy виріс зі скрипта
> [sl5net/build-obs-nvenc-legacy](https://github.com/sl5net/build-obs-nvenc-legacy). Якби не він, не було б
> і цього проєкту. Дякуємо його автору! ♥

> [!NOTE]
> **Інтерфейс — українською й англійською.** Мова береться з системи (локаль `uk_*` — українська, будь-яка
> інша — англійська); змінити можна в **Додатково → Налаштування → Мова інтерфейсу**. Скрипт збирання, його
> звіти й `install.sh` говорять тією самою мовою.

<p align="center">
  <img src="docs/screenshots/home.png" alt="Головний екран: меню з чотирьох кроків, порада «Що далі» і стан системи" width="820">
</p>

## Навіщо це

Драйвери NVIDIA гілки **580xx** — останні, що підтримують відеокарти Maxwell, Pascal і Volta. Вони знають
NVENC API лише **до 13.0**, а OBS і ffmpeg з репозиторіїв Arch/Artix уже зібрані з SDK 13.1. Тож у
звичайному OBS апаратне кодування NVENC просто не вмикається, і стрім чи запис навантажують процесор.

OBS-Legacy збирає **офіційний OBS Studio** з приватною копією заголовків NVENC 13.0 (системні файли не
змінюються), офіційним CEF для Browser Source, вибраними плагінами й ffmpeg — усе **оптимізоване саме під
твоє залізо** — і ставить їх звичайними пакетами pacman з припискою `-legacy-580xx`:
`obs-studio-legacy-580xx`, `<плагін>-legacy-580xx`, `ffmpeg-legacy-580xx`. Вони лягають у ті самі місця,
що й OBS з репозиторію (`/usr`), тож pacman сам пропонує замінити `obs-studio` / `obs-studio-git`, а
плагіни з AUR ставляться зверху як завжди. Пакети встановлюються лише тоді, коли тест `obs-nvenc-test`
підтвердив, що NVENC справді працює.

## Що вміє

- **Веде за руку.** Головний екран показує, що вже готово і що робити далі: курсор стоїть на
  потрібному пункті, тож зазвичай досить натиснути Enter. У меню чотири кроки, решта сховано в
  «Додатково».
- **Пояснює й виправляє.** Після кожного запуску внизу з'являється картка «Що сталося / Що робити» з
  порадою від скрипта. Що можна виправити саме (поставити пакети, яких бракує, очистити кеш), —
  виправляється кнопкою «Виправити». Повний звіт відкривається прямо в програмі (`o`), шлях копіюється
  (`c`), а сам файл — у текстовому редакторі (`e`).
- **Читається в будь-якому терміналі.** Суцільний темний фон не дає тексту губитися в прозорих Kitty,
  Alacritty чи Konsole. Якщо фон не потрібен, його можна вимкнути в налаштуваннях.
- **Відкривається клацанням.** Ярлик «OBS-Legacy» у меню програм сам відкриває вікно термінала
  (kitty, konsole, alacritty, foot, wezterm, gnome-terminal, xterm та інші).
- **Зібрано під твоє залізо.** Програма сама визначає дистрибутив, процесор, відеокарту й драйвер і
  підбирає параметри збирання: прапорці компілятора саме під цей процесор (`-march=native`, напр. AVX на
  Xeon Ivy Bridge замість базового x86-64 пакетів з репозиторію), решту — з `makepkg.conf` твого
  дистрибутива; найновіші заголовки NVENC, які знає драйвер; CUDA-архітектуру відеокарти для
  CUDA-фільтрів ffmpeg (кілька відеокарт NVIDIA — найстаршу, щоб фільтри працювали на всіх). Під твій
  процесор зібрано й саму програму `obs-legacy`. Увесь профіль видно на етапі «Оптимізація під залізо»
  у звіті.
- **Справжні пакети pacman.** `obs-studio-legacy-580xx` «надає» `obs-studio`, тож плагіни з AUR, що його
  потребують, ставляться як завжди, а `pacman -Qs legacy-580xx` показує все, що поставив OBS-Legacy.
- **Browser Source вбудований.** Використовується офіційний Linux-архів CEF від OBS, а його SHA-256
  звіряється з офіційним хешем із вихідних кодів OBS. Без `obs-browser` збірка не встановиться.
- **Плагіни галочками.** [obs-multi-rtmp](https://github.com/sorayuki/obs-multi-rtmp) (стрім одразу на
  кілька платформ) увімкнено типово, ще 17 плагінів на вибір, серед них
  [RewardsTheater](https://github.com/gottagofaster236/RewardsTheater). Кожен збирається з вихідних кодів
  під твою збірку OBS і твій процесор, стає окремим пакетом (`obs-multi-rtmp-legacy-580xx`…) і
  оновлюється разом з OBS.
- **ffmpeg з NVENC.** `ffmpeg-legacy-580xx` — той самий ffmpeg, що в репозиторії (його PKGBUILD з Arch),
  але з NVENC для драйвера 580xx і під твій процесор: NVENC запрацює і в «Власний вивід (FFmpeg)» в OBS, і
  в Kdenlive, і в `ffmpeg` у терміналі. Перезбирається лише тоді, коли в репозиторії виходить новий ffmpeg.
- **Замінює OBS з репозиторію надовго.** Після оновлень системи pacman-хук із пакета OBS підкаже, коли
  OBS пора перезібрати (або коли вийшов новий ffmpeg). Попередні набори пакетів зберігаються, тож відкат —
  без перезбирання.
- **Повідомляє про оновлення.** За бажанням фонова перевірка раз на 12 годин дивиться, чи вийшли новий
  OBS, нові релізи плагінів, новіший ffmpeg у репозиторії чи новий OBS-Legacy, а після оновлень системи — чи власна збірка OBS
  знаходить свої бібліотеки. Кожна новина приходить один раз, сповіщенням на робочому столі; клацання
  відкриває OBS-Legacy. Працює як служба користувача твоєї init-системи (systemd, dinit, OpenRC, runit чи
  s6) або з автозапуску сесії, без sudo.
- **Обережний із системою.** Система змінюється лише через pacman (`pacman -S` для пакетів збірки,
  `pacman -U` для пакетів OBS-Legacy), лише після твого «y» і ніколи з `--noconfirm`. Повне оновлення
  системи (`pacman -Syu`) скрипт не запускає, а збирання й пакування йдуть без root.

## Встановлення

Потрібні Artix або Arch і драйвер NVIDIA 580xx. Вибери один із двох способів.

**Готовий пакет.** До кожного [релізу](https://github.com/YellowHearth1/OBS-Legacy/releases) додано
`obs-legacy-<версія>-1-x86_64.pkg.tar.zst`, зібраний з вихідних кодів релізу в чистому контейнері Arch
([`packaging/PKGBUILD`](packaging/PKGBUILD)). Rust не потрібен: завантаж пакет і постав його pacman-ом.

```bash
sudo pacman -U obs-legacy-0.0.2-1-x86_64.pkg.tar.zst
```

Програма ляже в `/usr/bin/obs-legacy`, а в меню програм з'явиться ярлик **OBS-Legacy**. Щоб дізнаватися про
нові версії, увімкни [сповіщення про оновлення](#сповіщення-про-оновлення) («Додатково → Сповіщення про
оновлення»); оновлюйся пакетом з нового релізу так само.

**З вихідних кодів** (потрібні `git` і Rust; якщо Rust немає, установник запропонує поставити пакет `rust`):

```bash
git clone https://github.com/YellowHearth1/OBS-Legacy.git
```

```bash
cd OBS-Legacy && ./install.sh
```

Установник збирає `obs-legacy` під твій процесор, кладе його в `~/.local/bin` і додає ярлик **OBS-Legacy** у
меню програм (за бажанням і на робочий стіл). Ще він запропонує ввімкнути
[сповіщення про оновлення](#сповіщення-про-оновлення). Далі просто клацни по ярлику або набери
`obs-legacy` у терміналі.

Вибирай щось одне: копія в `~/.local/bin` перекриває ту, що з пакета. Переходиш на пакет — спершу
`./install.sh --uninstall`.

## Як користуватися

1. **Перевірити систему.** Драйвер, NVENC, пакети, завантаження вихідних кодів OBS і CEF. Нічого не
   змінює, триває 5–15 хв.
2. **Плагіни** — за бажанням. Галочками обери, що зібрати разом з OBS.
3. **Встановити / оновити OBS.** Збирання під твоє залізо (30–90 хв; уперше з ffmpeg — довше, повторне
   — швидше), тест NVENC, перевірка бібліотек, пакування і `pacman -U`. pacman спитає, чи замінити
   `obs-studio-git` (або `obs-studio`, `ffmpeg`, плагіни з AUR) — відповідай `y`. Пароль sudo/doas і
   відповіді `y`/`n` вводяться прямо у вікні програми, а жовта підказка внизу скаже, коли саме.
4. **Запускай OBS як завжди**: з меню програм, командою `obs` або пунктом «Запустити OBS».

<p align="center">
  <img src="docs/screenshots/run.png" alt="Збирання: етапи ліворуч, живий вивід праворуч, прогрес компіляції внизу" width="820">
</p>

Оновлення — той самий пункт «Встановити / оновити OBS». Коли OBS-Legacy уже встановлено, він відкриває
«Оновити OBS?», і типова кнопка там — «Оновити»: програма спершу перевіряє, що нового (OBS, релізи
плагінів, ffmpeg, профіль заліза, бібліотеки після оновлення системи), і збирає, лише якщо є що
оновлювати, — інакше каже «Оновлень немає» і нічого не змінює. Картка результату показує, що саме
змінилося, наприклад `OBS 33.0.0 → 33.0.1`. Панель «Що далі» підкаже, коли вийшла нова версія.

Друга кнопка — «Перезібрати» (клавіша `R`; або «Додатково → Перезібрати OBS») — збирає все з нуля й ставить
знову, навіть якщо нового немає: коли збирання обірвалося (вимкнулося світло, закрилося вікно) чи OBS
поводиться дивно. Якщо програма знайде обірване збирання, «Що далі» сама запропонує перезібрати.

| Клавіші | Дія |
|---|---|
| `↑↓`, `Enter` | вибрати й відкрити |
| `Esc` / `q` | назад (у меню — вихід) |
| `Enter` на картці результату | виправити те, що запропонував скрипт (або повернутися в меню) |
| `o` · `c` · `e` | переглянути звіт · скопіювати шлях · відкрити в програмі |
| `d` | стан системи коротко або докладно |
| `Пробіл` | галочка плагіна |
| `PgUp` / `PgDn` | прокрутка виводу й звітів |
| `Ctrl+C` ×2 | перервати завдання |
| `?` / `F1` | довідка |
| миша | клац — вибрати, ще клац — відкрити, коліщатко — прокрутка |

## Якщо щось пішло не так

Після кожного запуску програма коротко пише, що сталося і що з цим робити. Поради дає сам скрипт:
яких пакетів бракує, як звільнити місце, що перевірити в драйвері. Якщо це можна виправити одразу,
головна кнопка картки — «Виправити»: наприклад, вона ставить пакети для збірки, після чого програма
пропонує перевірити систему ще раз, і всі етапи вже з галочками.

<p align="center">
  <img src="docs/screenshots/result.png" alt="Картка результату з кнопкою «Виправити»: на що зважити, що далі, кнопки звіту" width="820">
</p>

Кожен запуск зберігає повний звіт. Список усіх звітів — у «Додатково → Звіти»: там видно, чим
закінчився кожен запуск, а у вбудованому переглядачі `n` і `p` стрибають між попередженнями й
помилками.

<p align="center">
  <img src="docs/screenshots/reports.png" alt="Список звітів із результатом кожного запуску" width="820">
</p>

Поточний OBS не зламається: нові пакети ставляться лише після всіх перевірок. Не сподобалася нова
версія — «Відкат на іншу збірку»: він поставить попередній набір пакетів. Вимкнулося світло посеред
збирання — «Перезібрати OBS»: він почне з чистого кешу збірки й прибере застарілі блокування git. Якщо
обірвало сам pacman, програма підкаже, як прибрати його блокування (`/var/lib/pacman/db.lck`). Не
допомогло — створи issue і додай звіт.

На запитання `[y/N]` відповідай латиницею `y` чи `n` або словом «так» / «ні». Якщо ввімкнена українська
розкладка, клавіша `Y` дає «н»: скрипт це помітить і перепитає, а не скасує. Усе ж скасував встановлення на
останньому запитанні? Зібрані пакети нікуди не дінуться: «Встановити / оновити OBS» поставить цей готовий
набір без перезбирання, а головний екран про нього нагадає.

## Питання й відповіді

**У мене прозорий Kitty, і текст губиться на шпалерах.** Інтерфейс має власний темний фон, тож
прозорість його не зачіпає. Хочеш бачити тло термінала — вимкни «Налаштування → Суцільний фон».

**Перевірка пише «бракує пакетів» і пропускає пробну конфігурацію CMake.** Натисни «Виправити» (або пункт
«Встановити пакети»): програма поставить лише ці пакети через pacman, а їхні залежності pacman додасть
сам, без оновлення системи. Потім вона запропонує перевірити систему ще раз, і всі етапи будуть ✓.
Можна нічого не робити: «Встановити / оновити OBS» теж поставить ці пакети.

**Чи треба самому видаляти `obs-studio` / `obs-studio-git`?** Ні. `obs-studio-legacy-580xx` конфліктує з
будь-яким пакетом OBS, тож під час встановлення pacman спитає, чи замінити його, — відповідай `y`. Так само
з AUR-версіями вибраних плагінів і з `ffmpeg`. Повернутися до OBS з репозиторію: «Додатково → Видалити
OBS-Legacy», потім `sudo pacman -S obs-studio`.

**Чи можна ставити інші плагіни з AUR?** Так. OBS-Legacy ставить OBS у `/usr`, як пакет з репозиторію, і
«надає» `obs-studio`, тож плагіни з AUR ставляться на нього як завжди.

**Наскільки це швидше?** Найбільший виграш — сам NVENC: кодування переходить з процесора на відеокарту.
Прапорці під процесор додатково прискорюють OBS, плагіни й ffmpeg (напр., використовується AVX там, де
пакети з репозиторію розраховані лише на SSE2); точний виграш залежить від процесора й сцени. Профіль
видно у звіті й у «Твоя система» (клавіша `d`). Зважай: пакети під `native` працюють лише на цьому
процесорі; після його заміни програма запропонує перезібрати, а «Налаштування → Під який процесор» має
переносні варіанти.

**Чи все перезбирається під мій процесор?** Під процесор збираються сам OBS, його плагіни, ffmpeg
(`ffmpeg-legacy-580xx`) і програма `obs-legacy`. Qt, x264, mbedTLS та інші системні бібліотеки лишаються з
репозиторію: найважчі з них (x264, асемблерний код ffmpeg, малювання в Qt) і так під час роботи вибирають
SIMD-інструкції під процесор, а перезбирати всю систему не варто. CEF (рушій Browser Source) — готовий архів
від OBS Project; з нього лише прибирається налагоджувальна інформація (близько 2,1 ГБ → 550 МБ).

**Що робити після `pacman -Syu`?** Нічого, поки не з'явиться підказка. Після кожного оновлення системи
pacman-хук із пакета OBS перевіряє бібліотеки пакетів OBS-Legacy й пише, якщо їх пора перезібрати
(«Встановити / оновити OBS»), а також — якщо в репозиторії вийшов новий ffmpeg.

**У мене драйвер новіший за 580xx.** Тоді NVENC працює і в OBS з репозиторію, і OBS-Legacy тобі,
найімовірніше, не потрібен. Програма скаже про це на головному екрані.

**Нова версія OBS не працює з моїм драйвером.** `obs-nvenc-test` таку збірку не пропустить. Зафіксуй
останню робочу версію в «Налаштування → Версія OBS» (наприклад, `33.0.0`).

## Плагіни

Обираються в програмі або рядком `OBS_PLUGINS=a,b` у файлі налаштувань (`none` — без плагінів).
Якщо зняти галочку, пакет плагіна `-legacy-580xx` прибереться під час наступного «Встановити / оновити OBS»
(pacman спершу спитає): його зібрано під попередню збірку OBS.

<p align="center">
  <img src="docs/screenshots/plugins.png" alt="Вибір плагінів галочками" width="820">
</p>

| id | Плагін | Що робить |
|---|---|---|
| `obs-multi-rtmp` | [Multiple RTMP Outputs](https://github.com/sorayuki/obs-multi-rtmp) | Стрім одночасно на кілька платформ (Twitch, YouTube, Kick…) з окремими налаштуваннями кожного виходу. **Увімкнено типово** |
| `move-transition` | [Move Transition](https://github.com/exeldro/obs-move-transition) | Плавна анімація положення, розміру й властивостей джерел між сценами та фільтри руху |
| `source-record` | [Source Record](https://github.com/exeldro/obs-source-record) | Запис окремих джерел або сцен в окремі файли паралельно з основним записом |
| `shaderfilter` | [Shaderfilter](https://github.com/exeldro/obs-shaderfilter) | Фільтри на власних шейдерах: свічення, розмиття, хромакей, спотворення та десятки готових ефектів |
| `vertical-canvas` | [Aitum Vertical Canvas](https://github.com/Aitum/obs-vertical-canvas) | Окреме вертикальне полотно 9:16 для TikTok / Shorts / Reels |
| `pipewire-audio-capture` | [PipeWire Audio Capture](https://github.com/dimtpap/obs-pipewire-audio-capture) | Захоплення звуку окремих програм і пристроїв через PipeWire |
| `composite-blur` | [Composite Blur](https://github.com/FiniteSingularity/obs-composite-blur) | Якісні фільтри розмиття: гаусове, кругове, рухом, пікселізація, з масками |
| `advanced-masks` | [Advanced Masks](https://github.com/FiniteSingularity/obs-advanced-masks) | Маски-форми й градієнтні маски для джерел |
| `stroke-glow-shadow` | [Stroke Glow Shadow](https://github.com/FiniteSingularity/obs-stroke-glow-shadow) | Обведення, свічення й тіні для будь-якого джерела |
| `retro-effects` | [Retro Effects](https://github.com/FiniteSingularity/obs-retro-effects) | Ретро-ефекти: CRT, VHS, глітч, дизеринг, хроматична аберація |
| `scale-to-sound` | [Scale to Sound](https://github.com/dimtpap/obs-scale-to-sound) | Джерело масштабується в такт звуку |
| `downstream-keyer` | [Downstream Keyer](https://github.com/exeldro/obs-downstream-keyer) | Шар поверх усіх сцен (логотип, титри), який не зникає при перемиканні сцен |
| `transition-table` | [Transition Table](https://github.com/exeldro/obs-transition-table) | Свій перехід для кожної пари сцен |
| `freeze-filter` | [Freeze Filter](https://github.com/exeldro/obs-freeze-filter) | Заморожування кадру джерела фільтром або гарячою клавішею |
| `replay-source` | [Replay Source](https://github.com/exeldro/obs-replay-source) | Миттєвий повтор (instant replay) прямо в сцені |
| `gradient-source` | [Gradient Source](https://github.com/exeldro/obs-gradient-source) | Джерело-градієнт з кількома кольорами |
| `waveform` | [Waveform](https://github.com/phandasm/waveform) | Аудіовізуалізатор (спектр або хвиля) як джерело |
| `rewards-theater` | [RewardsTheater](https://github.com/gottagofaster236/RewardsTheater) | Глядачі замовляють відео чи звуки на стрімі за бали каналу Twitch (черга, модерація, випадкове відео з VLC-джерела) |

Як це влаштовано:

- береться найновіший тег-реліз плагіна. Версію можна зафіксувати рядком
  `PLUGIN_REF_<id>=тег` (дефіси в `id` замінити на `_`), напр. `PLUGIN_REF_obs_multi_rtmp=0.7.4.5`;
- плагін збирається з вихідних кодів проти `libobs` і `obs-frontend-api` саме цієї збірки OBS, з тими
  самими прапорцями під процесор, і стає окремим пакетом з назвою як в AUR плюс приписка:
  `obs-multi-rtmp-legacy-580xx`, `obs-move-transition-legacy-580xx`, `rewards-theater-obs-legacy-580xx`…
  Він «надає» назву з AUR і конфліктує з AUR-версіями, тож pacman запропонує їх замінити;
- якщо плагін не зібрався (наприклад, ще не підтримує нову версію OBS), OBS встановиться **без нього**,
  з попередженням, а попередній набір пакетів з ним лишається для відкату. Щоб у такому разі скасувати
  встановлення, є `PLUGINS_STRICT=1`.

> [!NOTE]
> Усі 18 плагінів збираються з вихідних кодів з OBS 33.0.1 на Artix (перевірено в жовтні 2026), а
> `obs-multi-rtmp`, `pipewire-audio-capture`, `composite-blur` і `waveform` перевірено й у самому OBS. Новий
> реліз плагіна чи OBS однаково може зламати збирання — тоді OBS встановиться без цього плагіна. Якщо так
> сталося, напиши issue з журналом `~/.cache/obs-studio-nvenc-artix/plugins/<id>/build.log`.

## Налаштування

Файл `~/.config/obs-legacy/settings.env` (рядки `KEY=VALUE`) редагується з програми, але діє і для
скрипта без неї. Змінна середовища завжди має пріоритет над файлом.

| Ключ | Типово | Що означає |
|---|---|---|
| `OBS_REF` | `stable` | найновіший стабільний реліз; або тег (`33.0.0`), гілка чи коміт |
| `OBS_PLUGINS` | `obs-multi-rtmp` | плагіни через кому; `none` — без плагінів |
| `PLUGIN_REF_<id>` | — | зафіксувати версію плагіна |
| `CPU_TUNING` | `native` | під який процесор збирати: `native` (цей, найшвидше), `generic` (переносно, як у репозиторії), `x86-64-v2` / `v3` / `v4` |
| `BUILD_FFMPEG` | `1` | збирати `ffmpeg-legacy-580xx` (`0` — лишити системний ffmpeg) |
| `KEEP_VERSIONS` | `2` | скільки наборів пакетів тримати для відкату разом зі встановленим |
| `INSTALL_PACMAN_HOOK` | `1` | класти в пакет OBS перевірку бібліотек після оновлень системи (pacman-хук) |
| `UI_LANG` | `auto` | мова програми, скрипта і звітів: `auto` (за локаллю системи), `uk` або `en` |
| `UI_BACKGROUND` | `1` | суцільний фон інтерфейсу; `0` — фон термінала (лише для програми) |
| `BUILD_JOBS` | авто | потоки компіляції (авто ≈ 2 ГБ RAM на потік) |
| `PLUGINS_STRICT` | `0` | скасувати встановлення, якщо плагін не зібрався |
| `FORCE_CLEAN_BUILD` | `0` | щоразу збирати з нуля |
| `OPT_LEVEL` | `2` | рівень оптимізації компілятора: `2` (як у дистрибутива) або `3` |
| `NV_HEADERS_TAG` | `auto` | тег `nv-codec-headers`; `auto` — найновіший, який знає драйвер (580xx → `n13.0.19.1`) |
| `CEF_BUILD` | авто | версія CEF (типово — з вихідних кодів OBS) |
| `EXTRA_CMAKE_ARGS` | — | додаткові параметри CMake для OBS |

## Оновлення, відкат і видалення

- **Оновити OBS і плагіни** — «Встановити / оновити OBS» → «Оновити»: збирає, лише якщо є нове, і
  використовує попереднє збирання, тож швидше. «Перезібрати» (або «Додатково → Перезібрати OBS») — з нуля.
- **Оновити саму програму** — «Додатково → Оновити OBS-Legacy» (або `git pull && ./install.sh --update`
  у теці з репозиторієм). Ставив готовий пакет — постав пакет з нового релізу:
  `sudo pacman -U obs-legacy-<версія>-1-x86_64.pkg.tar.zst`.
- **Відкат** — «Додатково → Відкат на іншу збірку»: ставить збережений набір пакетів через `pacman -U`,
  без перезбирання.
- **Видалити** — «Додатково → Видалити OBS-Legacy»: `pacman -R` пакетів `*-legacy-580xx`, а
  `ffmpeg-legacy-580xx` замінюється системним ffmpeg (`pacman -S ffmpeg`). OBS з репозиторію потім — одна
  команда: `sudo pacman -S obs-studio`. Потім сама програма:

```bash
./install.sh --uninstall
```

Якщо ставив готовий пакет: `sudo pacman -R obs-legacy`.

### Сповіщення про оновлення

«Додатково → Сповіщення про оновлення» вмикає фонову перевірку; установник теж її пропонує. Вона
дивиться:

- чи вийшли новий OBS, нові релізи плагінів твоєї збірки, новий ffmpeg у репозиторії (тоді
  `ffmpeg-legacy-580xx` пора перезібрати) і новий OBS-Legacy — раз на 12 годин;
- чи не бракує пакетам OBS-Legacy бібліотек — після кожної транзакції pacman (та сама перевірка, що й у
  pacman-хука).

Кожна новина приходить один раз, сповіщенням на робочому столі, а його кнопка «Відкрити OBS-Legacy»
відкриває програму. Про зафіксовану версію OBS (`OBS_REF`) чи плагіна (`PLUGIN_REF_<id>`) сповіщень
немає.

Перевірка працює як служба користувача, без root. Програма бере перший варіант, який працює на твоїй
системі:

| Init-система | Служба | Примітки |
|---|---|---|
| systemd | `~/.config/systemd/user/obs-legacy-updates.service` | |
| dinit | `~/.config/dinit.d/obs-legacy-updates` | dinit користувача, його запускає userspawn (з elogind) чи turnstile (з seatd чи elogind); обидва читають `~/.config/dinit.d/boot.d` |
| OpenRC 0.60+ | `~/.config/rc/init.d/obs-legacy-updates` | служби користувача (`pam_openrc`) |
| runit, s6 | `obs-legacy-updates` у теці служб користувача | власний `runsvdir` чи `s6-svscan` користувача, напр. turnstile з runit: `~/.config/service` |
| служб користувача немає | `~/.config/autostart/obs-legacy-updates.desktop` | автозапуск графічної сесії (XDG) |

Перевірено на Artix з dinit (userspawn); решта init-систем підтримується, але на справжньому залізі ще не
перевірялася.

**Чи може служба зламати завантаження?** Ні. Це служба *користувача*: її запускає менеджер служб
користувача (`systemd --user`, `dinit --user`…) після входу в систему, а init системи її не читає, тож
завантажитися комп'ютеру вона не завадить. До того ж:

- перезапуски після падінь обмежено: systemd здається після 4 запусків за 10 хвилин, dinit — після
  3 перезапусків за 5 хвилин, OpenRC — після 3 за 10 хвилин; runit і s6 після кожного падіння чекають
  хвилину;
- найсуворіший тут dinit: *зламаний* опис служби в `boot.d` не дає `dinit --user` запуститися взагалі, а з
  ним і решті служб користувача (скажімо, PipeWire). Тому OBS-Legacy перевіряє опис `dinit-check` ще до
  запису, `dinitctl enable` не вмикає опису, якого не може прочитати, а після оновлення опис, якого dinit
  не прийняв, замінюється попереднім;
- якщо щось таки піде не так, флешка не знадобиться: увійди в текстову консоль (Ctrl+Alt+F2) і виконай
  `obs-legacy --notifier disable` або видали вручну `~/.config/dinit.d/boot.d/obs-legacy-updates` і
  `~/.config/dinit.d/obs-legacy-updates`.

Без меню:

```bash
obs-legacy --notifier status
```

Інші команди: `--notifier enable [systemd|dinit|openrc|runit|s6|autostart]`, `--notifier disable`,
`--notifier test` (пробне сповіщення) і `--check-updates` (перевірити просто зараз). Журнал —
`~/.local/state/obs-legacy/updates.log`.

Сповіщення надсилаються через `notify-send` (пакет `libnotify`), а якщо його немає — через `gdbus`, до
служби сповіщень твого робочого столу. У GNOME, KDE і Xfce вона вбудована; у sway чи Hyprland — mako,
dunst або swaync. Якщо ні init-система, ні віконний менеджер нічого за тебе не запускають (немає служб
користувача й `~/.config/autostart`), додай `obs-legacy --watch` в автозапуск віконного менеджера.

## Скрипт без програми

Уся робота — це bash-скрипт [`scripts/build-obs-nvenc-legacy-artix.sh`](scripts/build-obs-nvenc-legacy-artix.sh).
Програма вбудовує його в себе й розпаковує в `~/.local/share/obs-legacy/`. Скрипт можна запускати й
напряму:

```bash
~/.local/share/obs-legacy/build-obs-nvenc-legacy-artix.sh help
```

Команди: `check`, `install` (`update`), `package` (лише зібрати пакети, нічого не встановлювати), `deps`
(лише пакети для збірки), `status`, `rollback [НАБІР]`, `uninstall`, `clean`, `plugins`. Скрипт говорить тією
самою мовою, що й програма (`UI_LANG`, інакше локаль системи). Переносні пакети для іншого комп'ютера:
`CPU_TUNING=generic … package`.

## Де що лежить

| Що | Де |
|---|---|
| Встановлені пакети | `pacman -Qs legacy-580xx`; файли — у `/usr`, як в OBS з репозиторію |
| Відомості про збірку | `/usr/share/obs-legacy/` (`BUILDINFO.txt`: версії, профіль заліза; `plugins/`, `ffmpeg.txt`) |
| Набори пакетів для відкату | `~/.local/share/obs-studio-nvenc-artix/packages/<мітка>/` |
| pacman-хук | `/usr/share/libalpm/hooks/obs-legacy.hook` → `/usr/lib/obs-legacy/check-libs` (у пакеті OBS) |
| Звіти | `~/.cache/obs-studio-nvenc-artix/reports/` |
| Кеш збирання | `~/.cache/obs-studio-nvenc-artix/` |
| Копії налаштувань OBS | `~/.local/share/obs-studio-nvenc-artix/config-backups/` |
| Налаштування OBS-Legacy | `~/.config/obs-legacy/settings.env` |
| Сповіщення про оновлення | стан і журнал `~/.local/state/obs-legacy/`; служба — див. [вище](#сповіщення-про-оновлення) |
| Програма | `~/.local/bin/obs-legacy`, ярлик `~/.local/share/applications/obs-legacy.desktop`; з готового пакета — `/usr/bin/obs-legacy`, `/usr/share/applications/obs-legacy.desktop` |

## Розробка

```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

`target/` розростається до кількасот мегабайтів. `./install.sh` прибирає його після встановлення програми, а
після власних збирань звільни місце командою `cargo clean`.

```bash
OBS_LEGACY_SCRIPT="$PWD/dev/fake-build.sh" cargo run
```

`dev/fake-build.sh` імітує вивід справжнього скрипта (етапи, поради, запитання, «пароль», прогрес ninja і
curl, звіт), нічого не змінюючи. `FAKE_FAIL=1` імітує помилку, `FAKE_FAST=1` прибирає паузи.
`dev/screenshot.py` робить PNG зі знімка `tmux capture-pane` — так знято скріншоти в цьому README, а
`dev/make-assets.py` збирає іконку й банер.

Будова: `src/runner.rs` (скрипт у псевдотерміналі), `src/analyzer.rs` (етапи, поради й прогрес з
виводу), `src/advice.rs` (порада «Що далі»), `src/reports.rs` (звіти), `src/ui/` (екрани). Докладно — у
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), план перевірки на справжньому залізі — у
[`docs/TESTING.md`](docs/TESTING.md).

## Версії

Поточна версія — **0.0.2**, перший публічний випуск: пакети pacman (`obs-studio-legacy-580xx` і решта),
збирання під твоє залізо, ffmpeg з NVENC, сповіщення про оновлення, «Оновити» і «Перезібрати». Список
змін — у [Releases](https://github.com/YellowHearth1/OBS-Legacy/releases). Версія програми —
`obs-legacy --version`, версія скрипта збирання — дата (`2026.10.31.1`).

## Використане ПЗ і подяки

OBS-Legacy лише зводить докупи чужу працю. Дякуємо всім, завдяки кому це можливо.

**Натхнення.** Проєкт виріс зі скрипта
[sl5net/build-obs-nvenc-legacy](https://github.com/sl5net/build-obs-nvenc-legacy): він показав, що OBS,
зібраний з власною копією заголовків NVENC, працює на драйверах 580xx. Якби не він, не було б і
OBS-Legacy.

**Збирається з вихідних кодів на твоєму комп'ютері.** OBS-Legacy завантажує це з офіційних джерел і не
поширює далі:

| Проєкт | Навіщо | Ліцензія |
|---|---|---|
| [OBS Studio](https://github.com/obsproject/obs-studio) | сама програма, `obs-studio-legacy-580xx` | GPL-2.0-or-later |
| [Chromium Embedded Framework](https://github.com/chromiumembedded/cef) | Browser Source: збірка CEF, яку публікує OBS Project, з перевіркою SHA-256 | BSD-3-Clause |
| [FFmpeg](https://ffmpeg.org) ([вихідні коди](https://git.ffmpeg.org/ffmpeg.git)) | `ffmpeg-legacy-580xx` | LGPL-2.1-or-later; GPL-3.0 у конфігурації Arch |
| [Пакет ffmpeg з Arch Linux](https://gitlab.archlinux.org/archlinux/packaging/packages/ffmpeg) | рецепт (PKGBUILD) для `ffmpeg-legacy-580xx`: ті самі параметри, що й у системного ffmpeg | 0BSD |
| [nv-codec-headers](https://github.com/FFmpeg/nv-codec-headers) | заголовки NVENC API під драйвер (для 580xx — 13.0) | MIT |

**Плагіни з каталогу** (ліцензії — як їх визначає GitHub; подробиці — у кожному репозиторії):

| Плагін | Автор | Ліцензія |
|---|---|---|
| [Multiple RTMP Outputs](https://github.com/sorayuki/obs-multi-rtmp) | sorayuki | GPL-2.0 |
| [Move Transition](https://github.com/exeldro/obs-move-transition) | Exeldro | GPL-2.0 |
| [Source Record](https://github.com/exeldro/obs-source-record) | Exeldro | GPL-2.0 |
| [Shaderfilter](https://github.com/exeldro/obs-shaderfilter) | Exeldro | GPL-2.0 |
| [Aitum Vertical Canvas](https://github.com/Aitum/obs-vertical-canvas) | Aitum | GPL-2.0 |
| [PipeWire Audio Capture](https://github.com/dimtpap/obs-pipewire-audio-capture) | dimtpap | GPL-2.0 |
| [Composite Blur](https://github.com/FiniteSingularity/obs-composite-blur) | FiniteSingularity | GPL-2.0 |
| [Advanced Masks](https://github.com/FiniteSingularity/obs-advanced-masks) | FiniteSingularity | GPL-2.0 |
| [Stroke Glow Shadow](https://github.com/FiniteSingularity/obs-stroke-glow-shadow) | FiniteSingularity | GPL-2.0 |
| [Retro Effects](https://github.com/FiniteSingularity/obs-retro-effects) | FiniteSingularity | GPL-2.0 |
| [Scale to Sound](https://github.com/dimtpap/obs-scale-to-sound) | dimtpap | GPL-2.0 |
| [Downstream Keyer](https://github.com/exeldro/obs-downstream-keyer) | Exeldro | GPL-2.0 |
| [Transition Table](https://github.com/exeldro/obs-transition-table) | Exeldro | GPL-2.0 |
| [Freeze Filter](https://github.com/exeldro/obs-freeze-filter) | Exeldro | GPL-2.0 |
| [Replay Source](https://github.com/exeldro/obs-replay-source) | Exeldro | GPL-2.0 |
| [Gradient Source](https://github.com/exeldro/obs-gradient-source) | Exeldro | GPL-2.0 |
| [Waveform](https://github.com/phandasm/waveform) | phandasm | GPL-3.0 |
| [RewardsTheater](https://github.com/gottagofaster236/RewardsTheater) | gottagofaster236 | GPL-3.0 |

**Програму `obs-legacy`** написано на Rust з такими крейтами (і їхніми власними залежностями, як-от
crossterm, — див. `cargo tree`):

| Крейт | Навіщо | Ліцензія |
|---|---|---|
| [ratatui](https://github.com/ratatui/ratatui) | інтерфейс у терміналі | MIT |
| [vt100](https://github.com/doy/vt100-rust) | вбудований термінал, що показує вивід скрипта | MIT |
| [portable-pty](https://github.com/wezterm/wezterm/tree/main/pty) (з WezTerm) | запускає скрипт у псевдотерміналі, щоб працювали `sudo` і запитання pacman | MIT |
| [anyhow](https://github.com/dtolnay/anyhow) | обробка помилок | MIT OR Apache-2.0 |
| [libc](https://github.com/rust-lang/libc) | системні виклики (`flock`, `setsid`, `dlopen`…) | MIT OR Apache-2.0 |

**А ще:**

- [pacman і makepkg](https://gitlab.archlinux.org/pacman/pacman) збирають і ставлять пакети; назви пакетів з
  [AUR](https://aur.archlinux.org) ідуть у `provides`/`conflicts`, тож pacman сам пропонує замінити версії з
  AUR;
- з [Phosphor Icons](https://github.com/phosphor-icons/core) (MIT) і
  [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) (OFL-1.1) складено іконку й банер;
- код написав [Claude Code](https://claude.com/claude-code) — ШІ-асистент для програмування від Anthropic
  (див. попередження на початку).

## Ліцензія

[Apache-2.0](LICENSE). OBS Studio, CEF, FFmpeg і плагіни мають власні ліцензії (див.
[Використане ПЗ і подяки](#використане-пз-і-подяки)). OBS-Legacy не поширює їхні бінарні файли, а лише
збирає їх з офіційних джерел на твоєму комп'ютері.

NVIDIA, GeForce і NVENC — торгові марки NVIDIA Corporation; OBS і OBS Studio — торгові марки OBS Project.
OBS-Legacy — незалежний проєкт, не пов'язаний із NVIDIA чи OBS Project і не схвалений ними. В іконці й
банері немає логотипів NVIDIA й OBS: їх зібрано (`dev/make-assets.py`) з іконки «відеокарта» набору
[Phosphor Icons](https://github.com/phosphor-icons/core) (MIT, [ліцензія](assets/third-party/LICENSE-phosphor.txt))
і шрифту JetBrains Mono (SIL OFL 1.1).

