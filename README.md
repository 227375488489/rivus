# Rivus

Приватный браузер для Windows: собственный движок для чтения, полноценный Chromium-совместимый режим для современных сайтов, блокировка трекеров на сетевом уровне и изоляция хранилищ по контейнерам.

<p align="center"><img src="docs/logo.svg" width="96" alt="Rivus"></p>

## Что уже работает

- **Современные сайты** — YouTube, Google, поиск, почта, банки и SPA открываются в режиме совместимости на WebView2 (Chromium): JavaScript, cookie, формы, видео.
- **Собственный движок Rivus** — HTML/CSS-рендерер на Rust без JavaScript. Используется для режима чтения и страниц, которым не нужен скрипт.
- **Контейнеры** — у каждого контейнера отдельный профиль WebView2: cookie, localStorage, IndexedDB и кэш не пересекаются. Личный, рабочий, «приватный» — сколько нужно.
- **Одноразовые контейнеры** — incognito-профиль, каталог удаляется при закрытии.
- **Щит** — локальный фильтрующий прокси отсекает рекламные, трекерные и майнинговые домены до установки TLS-соединения. Счётчик блокировок виден в тулбаре.
- **Очистка URL** — `utm_*`, `fbclid`, `gclid`, `yclid` и десятки других параметров удаляются при переходе.
- **Нулевая телеметрия** — Rivus не отправляет аналитику и не имеет серверов. Единственный необязательный запрос — проверка обновлений на GitHub (отключается в настройках).
- **Локальные данные** — история, закладки, пароли (ChaCha20-Poly1305 + Windows Credential Manager) и настройки лежат только на компьютере.
- **Автообновление** — подписанная контрольной суммой сборка `rivus.exe` из GitHub Releases.
- **Дизайн** — React + CSS: тёмная тема, синий акцент, кастомные вкладки, омнибокс, боковые панели, настройки всего.

## Честно о приватности

Rivus не обещает невозможного.

| Режим | JavaScript | Изоляция | Защита от fingerprinting |
|---|---|---|---|
| Совместимость (WebView2) | Да | Отдельный профиль на контейнер | DNT/GPC, единый User-Agent, блокировка tracker-хостов. Canvas/WebGL-шум **не** включён: Chromium-поверхность нельзя незаметно пропатчить без потери совместимости |
| Движок Rivus / Reader | Нет | В памяти вкладки | JavaScript и canvas/WebGL/AudioContext отсутствуют как поверхность |

Фильтрующий прокси видит только хост из `CONNECT`, без MITM. Поэтому блокировка работает по домену, а не по пути URL; CNAME-клоакинг на этой поверхности не раскрывается. Подробности — в [PRIVACY.md](PRIVACY.md).

## Сборка

Нужны:

- Windows 10/11 с WebView2 Runtime (входит в систему);
- Rust stable (MSVC toolchain) и Visual Studio Build Tools;
- Node.js 20+;
- Git.

```powershell
git clone https://github.com/227375488489/rivus.git
cd rivus

cd ui
npm ci
npm run build

cd ..
cargo build --release -p rivus
.\target\release\rivus.exe
```

Для разработки:

```powershell
cd ui
npm run dev        # интерфейс в браузере с мок-данными
npm run tauri dev  # если установлен Tauri CLI
```

Тесты ядра и движка:

```powershell
cargo test -p aegis-core
cargo test -p aegis-engine
```

## Где лежат данные

`%LOCALAPPDATA%\Rivus`

- `settings.json` — настройки, щит, темы;
- `history.jsonl`, `bookmarks.json`, `session.json`;
- `vault.bin` — пароли (ключ в Windows Credential Manager);
- `cookies.json` — только постоянные cookie собственного движка;
- `webviews\<контейнер>\` — профили WebView2;
- `filters\*.txt` — пользовательские списки блокировки.

Удалить всё можно в настройках: «Приватность» → «Стереть данные».

## Структура

```
crates/aegis-core    сеть, фильтры, cookie, DNS, URL, хранилище, vault
crates/aegis-engine  HTML/CSS-парсер, layout, paint, Reader Mode
crates/rivus         Tauri-бэкенд, WebView2-поверхность, прокси, автообновление
ui                   React + Vite интерфейс
```

Подробнее — [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Лицензия

MIT. См. [LICENSE](LICENSE).
