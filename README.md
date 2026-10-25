<div align="center">
  <img src="src-tauri/icons/icon.png" width="112" alt="Clip History icon" />
  <h1>Clip History</h1>
  <p>Локальная история буфера обмена для macOS</p>
  <p><strong>Rust · Tauri 2 · macOS</strong></p>
</div>

Clip History сохраняет скопированные тексты, изображения и файлы, чтобы их можно было быстро найти и скопировать снова. Окно открывается глобальным сочетанием клавиш и работает как компактная панель поверх рабочего пространства.

> Проект находится на ранней стадии разработки. Сейчас он ориентирован на macOS; приложение ещё не подписано и не нотарифицировано для распространения.

## Возможности

- История текстов, изображений и скопированных файлов.
- Поиск по истории, повторное копирование и очистка записей.
- Глобальное сочетание клавиш `⌘⇧V`.
- Настройки темы, размера и положения окна.
- Ограничения истории по количеству записей, сроку хранения и объёму.
- Пауза сбора и настройка запуска при входе в macOS.
- Данные истории хранятся локально на этом Mac.

## Сборка и запуск

Нужны macOS, Rust с Cargo и инструменты командной строки Xcode. Для запуска Tauri CLI:

```sh
cargo install tauri-cli --version '^2'
cargo tauri dev --manifest-path src-tauri/Cargo.toml
```

Чтобы собрать приложение:

```sh
cargo tauri build --manifest-path src-tauri/Cargo.toml --bundles app
```

Если Tauri CLI уже установлен другим способом, повторно устанавливать его не нужно.

## Использование

После запуска приложение остаётся в фоне. Нажмите `⌘⇧V`, чтобы показать историю, выберите запись, чтобы вернуть её в буфер обмена, или нажмите `Esc`, чтобы скрыть окно. Сочетание клавиш может конфликтовать с другими приложениями.

История содержит данные, которые пользователь копирует в буфер обмена. Записи хранятся локально и могут включать чувствительную информацию. Очистите историю или удалите данные приложения, если они больше не нужны.

## Структура проекта

- `src-tauri/` — Rust-приложение, интеграция с macOS, чтение буфера и настройки окна.
- `ui/` — интерфейс приложения на HTML, CSS и JavaScript.

## Лицензия

Код Clip History распространяется по [PolyForm Noncommercial License 1.0.0](LICENSE): разрешено личное и иное некоммерческое использование, коммерческое использование этой лицензией не предоставляется. Для него нужно отдельное разрешение правообладателя. Лицензии сторонних зависимостей действуют отдельно.

## Обратная связь

Баги и предложения можно отправлять через [Issues](../../issues).

---

<details>
<summary>English</summary>

Clip History is a local clipboard history app for macOS. It keeps copied text, images, and files available for quick search and reuse. Press `⌘⇧V` to open its floating history panel.

The project is in an early development stage. It is macOS-focused, and the app is not yet signed or notarized for distribution. Clipboard history stays on the Mac, but it can contain sensitive data.

### Build

Requirements: macOS, Rust/Cargo, and Xcode Command Line Tools.

```sh
cargo install tauri-cli --version '^2'
cargo tauri dev --manifest-path src-tauri/Cargo.toml
cargo tauri build --manifest-path src-tauri/Cargo.toml --bundles app
```

</details>
