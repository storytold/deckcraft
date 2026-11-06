# Study: DeckCraft — модель документа и пригодность к совместному редактированию

> Артефакт Phase 0. Пишется в клоне upstream: `<app>/upstream/docs/collab/study.md`
> на ветке `collab/study`. В шину уходит как `artifact` + краткое `proposal`.

- Проект: `deckcraft`
- Upstream: `https://github.com/storytold/deckcraft`
- Коммит: `159ed07eb3f0522a5dc97d7cf1858242a6638659` (2026-11-06, `main` + house rules)
- Дата: `2026-11-06`
- Автор: `deckcraft/envoy`

Все утверждения ниже проверены по коду этого коммита (ссылки `файл:строка`).
Цифры LOC/тестов получены `find … | xargs cat | wc -l` и `grep -c '#\[test\]'`.
Чего не измерял — помечено явно. Сборку и бенчмарки запустить не удалось:
git-зависимости (`craft-libs`, форк egui) отсутствуют в кэше cargo, сети нет,
`docs/benchmarks/baseline.json` в репозитории нет (`xtask/src/bench.rs:16`).

## 1. Карта репозитория

202k строк Rust, 24 крейта, 3 приложения, 2432 `#[test]` в 593 файлах.

| Crate / модуль | Слой | Назначение | LOC | Тесты |
|---|---|---|---|---|
| `crates/ui-egui` | L6 | интерфейс PowerPoint-стиля, canvas, панели, диалоги, control-канал, показ | 46 859 | 497 |
| `crates/engine` | L5 | `Session`, документы, выделение, undo, реестр команд (329 команд), инструменты указателя, AutoRecover, panic guard | 44 811 | 574 |
| `crates/pptx` | L3 | импорт/экспорт ECMA-376 (read 4 571, write 5 184) | 21 518 | 216 |
| `crates/render` | L3 | CPU-растр слайда (`vello_cpu`), Morph, переходы | 15 288 | 209 |
| `crates/model` | L1 | `Presentation`, слайды, мастера/layouts, фигуры, текст, таблицы, диаграммы, темы, анимации | 9 715 | 107 |
| `crates/anim` | L2 | состояние анимаций по времени, машина показа, переходы, Morph-матчинг | 6 182 | 98 |
| `crates/text` | L2 | шейпинг (HarfRust), bidi, переносы, autofit, hit-testing каретки | 5 964 | 97 |
| `crates/format` | L2 | нативный `.deckcraft` (zip + JSON + media), outline/markdown | 2 362 | — |
| `crates/pdf` | L4 | экспорт PDF через `craft-pdf` | 1 499 | — |
| `crates/media`, `fonts`, `geom`, `color`, `metafile`, `mcp` | L0–L2, L6 | медиа, шрифты, EMU/фигуры, цвет, EMF/WMF, MCP-сервер | 20 274 | — |
| `apps/deckcraft` | — | desktop (eframe/wgpu), control-сервер, логирование | 2 170 | — |
| `apps/deckcraft-cli` | — | `run`, `render`, `convert`, `mcp`, `app`, `bench` | 1 839 | — |
| `apps/deckcraft-web` | — | WASM-сборка (trunk), iframe-хост | 755 | — |

Точки входа: `apps/deckcraft/src/main.rs` (desktop + `--control PORT`),
`apps/deckcraft-web/src/web.rs` (WASM), `apps/deckcraft-cli` (CLI/`bench`/`mcp`),
`crates/mcp` (JSON-RPC 2.0 stdio). Слои проверяются `cargo xtask layers`
(`docs/architecture.md:14-30`), `unsafe_code = "forbid"` в workspace
(`Cargo.toml:76`).

## 2. Модель документа

- Корень — `Presentation` (`crates/model/src/lib.rs:974`): `slide_size`,
  `slides: Vec<Arc<Slide>>`, `masters: Vec<Arc<Master>>` (+ notes/handout master),
  `sections`, `custom_shows`, `media: Vec<MediaItem>`, `next_id`, `tags`,
  `kept_parts`, `raw_ext`.
- `Arc<Slide>` — не косметика: снимок всего документа для undo стоит клона
  `Vec<Arc<_>>` (`crates/model/src/lib.rs:8-12`), правка одного слайда — это
  `Arc::make_mut` на нём (`crates/model/src/lib.rs:1071-1073`, 328 вызовов
  `Arc::make_mut` по крейтам). То есть модель уже copy-on-write по слайдам.
- `Slide` (`:617`): `id`, `layout`, `name`, `shapes: Vec<Shape>` (**порядок
  вектора = z-order**), `background`, `hidden`, `transition`, `animations:
  Vec<Animation>` (плоский список, как в Animation Pane), `notes`, `comments`,
  `color_map`, `tags`, `raw_ext`.
- `Shape` (`:331`): `id`, `name`, `xfrm: Option<Xfrm>` (`None` — наследование
  от placeholder), `kind`, `geom`, `ph`, `fill`, `line`, `effects`, `style`,
  `text: Option<TextBody>`, `hidden`, `locked`, `text_box`, `descr`, гиперссылки,
  `tags`, `raw_ext`, `diagram`.
- `ShapeKind` (`:288`): `Shape`, `Picture`, **`Group { children: Vec<Shape>,
  child: Xfrm }`**, `Connector { start/end: Option<(ShapeId, u32)> }`, `Table`,
  `Chart`, `Media`, `Ink`, `Model3d`, `Opaque { xml, preview, label, parts }`
  (то, что прочитано, но не смоделировано — например SmartArt без fallback, OLE).
- Текст: `TextBody { body, list_style, paragraphs: Vec<Paragraph> }`;
  `Paragraph { level, props, runs: Vec<Run>, def_props, end_props }`;
  `Run { text, props, kind }` (`crates/model/src/text.rs:582-635,725-730`).
  Форматирование разрешается slide → layout → master → theme в момент рендера
  (`crates/model/src/resolve.rs`, `docs/architecture.md:33-40`).
- Таблицы: `Table`/`Cell` с собственным `TextBody` (`crates/model/src/table.rs`);
  диаграммы — `Chart` с `kept`-сайдкаром для round-trip.
- Медиа: `MediaItem { id, name, content_type, data: Arc<Vec<u8>> (serde skip),
  link }` (`:938`) — байты **не** попадают в JSON-манифест.
- Комментарии/заметки живут на слайде; автор и дата — строками (`:601-613`).
- Переход — на слайде (`Slide::transition`), анимации — `Slide::animations`,
  с ссылками на `ShapeId` и `trigger: Option<ShapeId>` (`crates/model/src/anim.rs:293-327`).

### Стабильные ID

- `SlideId`, `ShapeId`, `LayoutId`, `MasterId`, `MediaId` — обёртки над `u32`
  (`macro_rules! id_type`, `crates/model/src/lib.rs:42-58`), сериализуются как
  число.
- `Presentation::alloc_id` выдаёт «уникальный в колоде» id из `next_id`
  (`:1036`), `fix_next_id` поднимает счётчик выше всех прочитанных (`:1042`).
- **Но инвариант «уникален в колоде» не выполняется для импортированных
  фигур.** `pptx/src/read/shapes.rs:22-50`: `IdCtx::assign` сохраняет файловый
  `cNvPr@id`, если он в `2..2^30` и ещё не занят **в этой части**; счётчик свой
  на каждую часть (`per part`, `:26`). PowerPoint уникален только внутри
  слайда, поэтому два слайда легко получают `ShapeId(2)`; это прямо
  зафиксировано в комментарии `engine/src/cmd/arrange.rs:240`
  («shape ids repeat across slides»).
- `SlideId` при импорте выдаётся заново (`pptx/src/read.rs:597`,
  `SlideId(imp.alloc())`), а при экспорте `p:sldId@id = 256 + i` — то есть
  **позиция слайда** (`pptx/src/write.rs:1112`). Идентичность слайда в файле
  позиционная.
- Итого: `ShapeId` уникален в пределах слайда на время сессии, `SlideId` —
  только внутри сессии. Для совместной работы нужен собственный session-scoped
  ключ (см. `options.md`), а не файловый id.
- `Slide.shape`-ссылки в модели: коннекторы (`start`/`end`), комментарии
  (`Comment.shape`), анимации (`shape`, `trigger`), `last_duplicate`, undo-записи
  (`vertex_kind`), `media_status`, `gif_paused` в `Session` — всё по `ShapeId`
  в пределах слайда.

## 3. Команды и мутации

- Каждое видимое действие — `CommandSpec { id, label, menu, shortcut, params,
  enabled, run, journal, undoable }` (`crates/engine/src/cmd.rs:44-66`);
  реестр — `command_specs()` на `OnceLock`, 329 регистраций `cmd!`
  (`cmd.rs:161-195`, `grep -c 'cmd!('`), документация параметров — строка
  `params`, её же печатает `deckcraft-cli describe`.
- Единая точка входа: `Session::execute(&str, &serde_json::Value)`
  (`engine/src/lib.rs:636`). Через неё идут UI, `deckcraft-cli run`, JSON
  control-канал (`docs/control-protocol.md`) и MCP — «behave identically»
  (`docs/architecture.md:43-47`).
- `execute_unguarded` (`:642-699`) делает: проверка `enabled`, снимок `before =
  (uid, Arc<Presentation>, Selection)`, вложенность `depth` (внешняя команда —
  один undo-шаг), откат при ошибке, `push_undo` при `undoable && outer`,
  запись в `journal: Vec<(String, Value)>` при `journal && outer`.
- Мутация — `Session::edit` (`:727-738`): клон `(*st.doc)` (дешёвый: `Vec<Arc>`),
  правка, затем `arrange::refit_groups` и `connect::reroute`, `Arc::new(doc)`,
  `revision += 1`.
- Параметры команд сериализуемы (JSON), но **не все команды реплицируемы**:
  многие читают `Selection` (каретка `TextSel`, `selection.shapes`,
  `selection.slide`). Пример: `text.insert` берёт позицию из
  `selection.text` и делает `ed::delete(a,b)` + `ed::insert(body, at, text)`
  (`engine/src/cmd/text/editing.rs:75-100`). Позиция каретки — локальное
  состояние UI, у другого клиента она другая.
- Чистые примитивы для текста уже есть в модели и позиционны:
  `edit::insert(body, (para,char), text, props)`, `edit::delete(body, a, b)`,
  `edit::format(body, a, b, f)`, `insert_body`, `delete_word`
  (`crates/model/src/edit.rs:77,136,193,265,373`). Это готовое ядро для
  текстовых операций.
- Пересчёт после правки: `refit_groups` (`cmd/arrange.rs:238-315`) — **выводимое**
  состояние: если геометрия детей изменилась, а сама группа не двигалась, рамка
  группы и её `child`-пространство пересчитываются из объединения детей;
  `connect::reroute` пересчитывает маршруты коннекторов из концов.
- Стили/нумерация: `ListStyle`/`ParaProps`/`RunProps` наследуются, а не
  размножаются; правка форматирования сплитит `Run` по границам диапазона
  (`model/src/edit.rs:29-60`).

## 4. Undo/redo

- Стек — `History { undo: Vec<HistoryEntry>, redo: Vec<HistoryEntry>, limit,
  byte_limit, costs }` (`engine/src/lib.rs:176-186`), лимит 500 шагов
  (`:258`), бюджет `UNDO_BYTE_BUDGET = 256 MiB` (`:825`, вытеснение по
  `costs` в `push_undo` `:859-870`).
- В записи лежит **снимок всего документа**, а не команда и не инверсия:
  `HistoryEntry { label, doc: Arc<Presentation>, selection, vertex_kind }`
  (`:167-173`). `undo`/`redo` — это замена `st.doc` на сохранённый `Arc`
  (`cmd/edit.rs:18-19,66-71`), поэтому откат O(1) по указателям.
- Стек **на документ** (`DocState.history`), не глобальный; `Session` держит
  вектор документов (`:485-520`).
- Уже есть «одна команда = один шаг» и группа шагов через `depth`; жесты
  указателя и IME-композиция тоже сведены к одному шагу
  (`Interaction`, `Composition`, `:188-214`).
- **Важно для collab:** `push_undo` вызывается внутри `execute` автоматически
  для любой `undoable` команды. Значит применение чужой операции через
  `Session::execute` попадёт в локальный undo — нужен отдельный путь
  применения (см. §7) и своя инверсия на пользователя.
- Для rebase это удобно: откатить документ к любому `Arc<Presentation>` —
  это замена указателя, а повторное применение N непринятых команд стоит
  O(N × правка одного слайда), а не O(N × колода).

## 5. Сериализация и round-trip

- PPTX (`crates/pptx`, 21.5k строк): собственный ленивый читатель (неизвестные
  части пропускаются, битые логируются, размеры/глубина ограничены, zip-бомбы
  ограничены) и писатель, который PowerPoint открывает без repair
  (`docs/architecture.md:50-53`). Модель — `serde`-типы, файл — OPC-пакет.
- Что **не** моделируется и сохраняется как есть: `ShapeKind::Opaque { xml,
  preview, parts }` (OLE, SmartArt без fallback, ActiveX), `raw_ext` на
  презентации/слайде/фигуре, `Presentation.kept_parts` (custom XML, метка
  чувствительности, пользовательские свойства), `Slide.tags`,
  `Diagram.layout_xml/style_xml/colors_xml`, `Chart.kept`.
- Нативный `.deckcraft` (`crates/format`): zip с `mimetype`,
  `presentation.json` (модель **без** байтов медиа), `media/<id>`,
  `chart-parts/<index>` (`crates/format/src/lib.rs:1-5,73-100`). Это
  lossless-представление модели и готовый формат снапшота: JSON + отдельные
  бинарные объекты по `MediaId`.
- Read→write без правок: для смоделированного — стабильно (корпусные тесты
  `crates/pptx/tests/it/corpus*.rs` проверяют open → save → reopen), для
  немоделированного — через `Opaque`/`raw_ext`. Известные потери перечислены в
  `docs/gaps.md` и `docs/file-format-parity.md`; на нашем коммите открыты, в
  частности: медиа-опции воспроизведения (#74), макросы `.pptm/.potm/.ppsm`
  не распознаются, legacy `.ppt` не читается, SVG не рендерится (но фолбэк
  PNG показывается), прогрессивное сжатие EOT не рисуется.
- Экспорт: `p:sldId@id = 256 + i` (`write.rs:1112`), cNvPr-ids
  переназначаются по части, если id вне `2..i32::MAX` или дублируется
  (`write/shapes.rs:20-40`). **Следствие для collab:** после commit идентичность
  слайдов и фигур в файле не совпадает с сессионной; снапшот для сервера должен
  хранить модельные ID отдельно от файловых.
- Ограничения: `.potx`/`.ppsx` пишутся своим content type; шифрование не
  поддерживается; подписи/макросы — вне области.

## 6. Структурные правки и их цена

| Операция | Что ломается при конкурентной правке | Цена пересчёта |
|---|---|---|
| вставка/удаление слайда | `Vec<Arc<Slide>>` — порядок по индексу; `Slide.sections`, `custom_shows` (списки id), выделение по индексу | клон `Vec` указателей, O(#слайдов) |
| перестановка слайдов | `slide.move` адресует **индекс** `to`, «считая без перемещаемых» (`cmd/slide.rs:26-31`); при чужих вставках индекс уже другой | O(#слайдов) |
| вставка/удаление фигуры | `shapes: Vec<Shape>` = z-order; при удалении надо чистить `animations`, коннекторы, комментарии (это делается только в `slide.duplicate` через `remap_animations`, `cmd/edit.rs:230-237`) | клон `Slide` (`Arc::make_mut`), O(#фигур слайда) |
| z-order (`arrange.reorder`) | `{id, index}` — индекс среди сиблингов **внутри** группы (`cmd/arrange.rs:31-39`); две параллельные перестановки дают другой результат | O(#сиблингов) + проверка `stacking` |
| move/resize фигуры | `shape.move` принимает `dx/dy` (относительные) **или** `x/y` (абсолютные) (`cmd/shape.rs:61-66`) — относительные не идемпотентны | O(#фигур слайда) |
| группа: move/resize ребёнка | `refit_groups` пересчитывает рамку группы и `child` из объединения детей, если сама группа не менялась (`cmd/arrange.rs:238-315`). Двое двигают разных детей одной группы → у каждого свой пересчёт; применение «готовых» рамок группы от обоих клиентов даёт расхождение. Применение только **намерения** (xfrm ребёнка) и локальный пересчёт — сходится | O(#групп слайда) на каждую правку |
| group/ungroup | `arrange.ungroup` проецирует детей в слайд-пространство и пересобирает `Vec`; `regroup` опирается на **локальную историю undo** (`cmd/arrange.rs:373-386`) — на чужом клиенте не воспроизводится | O(#детей) |
| таблица: строка/столбец/ячейка | `Table` индексируется (row, col); объединённые ячейки — структура внутри; правка ячейки — отдельный `TextBody` | O(строк × столбцов) на перестройку |
| текст | позиции `(paragraph, char)` (`model/src/edit.rs:1-4`); вставка сплитит run, форматирование сплитит run по краям. Двое печатают в одном абзаце → нужен сдвиг позиций | O(длина абзаца) |
| стили/нумерация | наследование layout/master/theme: правка layout видна на всех слайдах этого layout | O(#слайдов с layout) при рендере, не при правке |
| анимации/переходы | `animations` — плоский упорядоченный список по `ShapeId`; удаление фигуры/слайда осиротит записи (сейчас чистится только в `duplicate`) | O(#анимаций слайда) |
| медиа | `MediaId` на `Arc<Vec<u8>>`; байты в снапшот/операцию не входят (`serde(skip)`) | 0 при правке, O(байт) при commit |
| комментарии | висят на `ShapeId`/координатах слайда | O(#комментариев) |

## 7. Существующие хуки для collab

- `Session` — фасад с `execute`/`edit`; всё уже заведено через команды, включая
  жесты (`Session::pointer`) и клавиши (`Session::key`).
- `Session.journal: Vec<(String, Value)>` — **уже журнал команд** (id + JSON
  параметры), режется по 10 000 (`engine/src/lib.rs:692-697`). На него опирается
  восстановление `file.adopt` (`cmd/file.rs:290-291`) и soak-тесты. Готовый
  прообраз журнала операций, но: без версий, без opId, включает локальные и
  UI-команды.
- `DocState.revision: u64` — локальный счётчик правок (инкремент в `edit`), может
  служить локальной `baseRevision`, но не глобальным порядком.
- `Selection` (`:148-164`) уже содержит всё для presence: `slide` (индекс),
  `slides: Vec<SlideId>`, `shapes: Vec<ShapeId>`, `text: Option<TextSel>`
  (anchor/caret как `(paragraph, char)`, `cell: Option<(r,c)>`, `notes`),
  `cells`, `target`. `ui.inspect` уже отдаёт выделение наружу.
- `engine/src/compare.rs` — готовый **diff** двух презентаций: слайды
  матчатся по `SlideId`, иначе по позиции; фигуры — по id, затем по имени;
  изменения на уровне слайда, accept/reject (`compare.rs:1-6,111-140`). Это
  прецедент «сверить две версии колоды» без CRDT и основа для UI расхождений.
- `guard::install_panic_hook` + `Session::panics_caught` — команда при панике
  откатывается (`docs/architecture.md:20`); soak-тесты (`engine/src/soak_tests.rs`)
  гоняют ~100-слайдовую колоду случайными командами, проверяя exactness
  undo/redo и save/reopen. Готовая headless-инфраструктура для тестов collab.
- Control-канал (`docs/control-protocol.md`): JSON-строки на `127.0.0.1`,
  `engine.execute`, `document.inspect`, `ui.render`, лимиты (16 соединений,
  16 MiB на строку, 60 с на UI). Синхронный `std::net`, без async/tokio.
- Сетевого кода и зависимостей нет вовсе: `grep` по `Cargo.toml` не находит
  ни `tokio`, ни `tungstenite`, ни `reqwest`; слова `collab`/`CRDT`/`WebSocket`
  в коде отсутствуют. Совместное редактирование — с нуля.

## 8. Производительность

- Честно: **замеров нет**. `docs/hardware-parity.md:55-59`: «Nothing is measured
  against PowerPoint yet: no benchmark of open time, render time per slide, show
  frame rate or memory exists». `xtask/src/bench.rs` есть, но
  `docs/benchmarks/baseline.json` в репозитории отсутствует, а без сети и
  git-зависимостей собрать `deckcraft-cli` здесь нельзя — цифр не будет.
- Что известно из кода:
  - растр слайда — CPU (`vello_cpu`), `render::render_slide(pres, index, opts)`
    (`crates/render/src/slide.rs:56`); GPU-путь — оценка 12–20 ч
    (`docs/hardware-parity.md`, «Estimates»);
  - UI кэширует растр слайда и миниатюры, `ui.inspect` отдаёт `renderMs`/`fps`
    (`ui-egui/src/control.rs:112`) — то есть встроенный измеритель уже есть;
  - soak-тест UI держит средний кадр < 250 мс и `thumb_count() <= 600`
    (`ui-egui/src/ui_soak_tests.rs:21,156`), это верхняя граница, а не норма;
  - правка стоит O(правка одного слайда) благодаря `Arc::make_mut`, плюс проход
    `refit_groups` по слайдам (`Arc::ptr_eq`-сравнение) и `undo_cost`;
  - стоимость отката/повторного применения — O(1) на указателях `Arc`, значит
    rebase-подход (§4) дешёвый по памяти и времени.
- Размер: типовой документ — колода на десятки слайдов с медиа; soak-тест
  использует ~100 слайдов (`docs/architecture.md:59-63`, `gaps.md:51-59`).
  Точного размера sample-колоды (`engine/src/sample.rs`, 138 строк) и
  максимального файла не измерял.
- WASM: `apps/deckcraft-web` — `#![forbid(unsafe_code)]`, eframe web-runner,
  WebGPU/WebGL2. Отличия от desktop перечислены в `apps/deckcraft-web/src/main.rs:1-14`:
  **нет TCP control-канала** (браузер не слушает сокеты), файлы приходят
  асинхронно через `Services::inbox`, сохранение — download или `?host=parent`.
  `rayon` используется только в кодеках (`crates/av1`, `vp9`, `hevc`, `pdf`), не в
  `engine`/`model`; `chrono` на wasm без `clock`. Потоков в движке нет —
  синхронный WebSocket-клиент на wasm требует отдельного решения (main-thread
  WS + канал событий в `inbox`).
- Веб-сборка кабинета: ~13 МБ, ~5 МБ сжатых (`deckcraft/INTEGRATION.md`).

## 9. Три варианта протокола (кратко)

| Вариант | Суть | Плюсы | Минусы | Риск |
|---|---|---|---|---|
| A | **Нормализованная команда + трансформация (OT)**: op = семантическая команда с явными адресами, сервер задаёт порядок, клиент трансформирует чужие операции против своих непринятых | ложится на существующий реестр; минимальный трафик | для 329 команд нужна матрица трансформаций; позиционные структуры (текст, z-order, слайды) требуют аккуратной математики | высокая сложность и цена ошибки |
| B | **Журнал команд с серверным порядком и rebase через откат/повтор**: клиент оптимистично применяет, при чужой операции откатывается к `Arc`-снимку и повторяет непринятые локальные команды; узкая трансформация только для позиций в абзаце | не нужна матрица OT; использует дешёвый `Arc`-откат; выводимое состояние (рамки групп, коннекторы, autofit) пересчитывается тем же кодом на каждой реплике | повторное выполнение N команд на каждую чужую операцию; нужен whitelist «чистых» команд и явные адреса вместо `Selection` | средний: цена rebase и «нереплицируемые» команды |
| C | **CRDT-lite по структурам**: RGA для списка слайдов, z-order и символов текста, LWW-регистры для скалярных свойств фигур, рамка группы выводится | сходимость без арбитража, offline «бесплатно» | на модель из `Vec<Shape>`/`Vec<Run>` ложится плохо; сервер уже нужен (JWT, права, commit); таблицы/диаграммы/анимации всё равно вне CRDT | средний-высокий: расхождение модельного и CRDT-представления |

Рекомендация и почему — в `options.md`.

## 10. Риски и открытые вопросы

1. **z-order и группы** (главный риск проекта). Двое двигают детей одной группы:
   у каждого свой `refit_groups`. Лечится тем, что операция несёт намерение
   (`xfrm` ребёнка), а рамка группы пересчитывается локально; проверять
   headless-тестом «двое двигают разных детей одной группы».
2. **Не идемпотентные параметры.** `shape.move {dx,dy}` и `text.insert`
   применяются дважды при повторе — нужен `opId` и дедупликация на сервере,
   а лучше нормализация в абсолютные значения.
3. **Команды читают `Selection`.** Их нельзя слать как есть; нужен слой
   «нормализованная операция» с явными адресами, иначе у получателя правка
   уйдёт в другое место.
4. **ID.** Файловые `ShapeId` уникальны только внутри слайда, `SlideId` —
   только в сессии. Нужен session-scoped ключ и надгробия для удалённых, иначе
   запоздавшая операция воскресит слайд/фигуру.
5. **Удаление слайда с объектами.** Запоздавшие операции по его `ShapeId`
   должны отклоняться, а не применяться к новому слайду с тем же id.
6. **Анимации и коннекторы** ссылаются на `ShapeId`; удаление фигуры сейчас
   чистит ссылки только в `slide.duplicate`. При совместном удалении нужна
   нормализация ссылок в операции, иначе на репликах останутся висячие ссылки
   (валидатор модели их не ловит).
7. **`regroup` опирается на локальную историю undo** — в совместном режиме
   поведение надо либо ограничить, либо сделать операцией с явными id.
8. **Медиа**: байты в модель не входят (хорошо), но `MediaId` выдаётся
   локально (`alloc_id`), поэтому вставка картинки двумя клиентами даёт разные
   id на один и тот же контент — нужен адрес по хэшу.
9. **Commit.** Экспорт перенумеровывает слайды и фигуры (`256 + i`,
   `IdMap::build`), а `Opaque`/`raw_ext` пишутся как есть. Значит commit
   обязан идти только из состояния сервера, иначе файл будет от одного клиента.
10. **Нет замеров** layout/рендера и размера колоды — бюджет задержки
    (например, «рендер слайда < N мс») придётся ставить по soak-тестам UI
    (< 250 мс на кадр) и уточнять на этапе реализации.
11. **Networking с нуля**: ни одной сетевой зависимости; на wasm нет потоков,
    control-канал в браузере недоступен. Транспорт придётся писать (или брать
    минимальный) под оба таргета.
12. **Права.** В модели есть `Shape.locked` и `viewer`-режим UI, но серверной
    проверки нет; `enabled`-проверки команд — только локальные.
