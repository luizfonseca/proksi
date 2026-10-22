# proksi-admin-ui

The admin UI of [Proksi](https://github.com/luizfonseca/proksi), served by its admin listener ([ADR 0008](https://docs.proksi.info/adrs/0008-admin-ui)): a Preact application styled with [daisyUI](https://daisyui.com) (on Tailwind), built into `dist/` and embedded by `build.rs`.

```bash
npm ci
npm run dev    # http://localhost:5173, with the API of a Proksi on 127.0.0.1:7555 (PROKSI_ADMIN to change it)
npm run build  # dist/, then build Proksi to embed it
```

A Proksi built without `dist/` serves a page saying how to build the UI. The crate is published with each release of Proksi, with its version and the built UI.

## Layout

```text
src/
  main.tsx, router.ts   the app and its routes
  api/                  the admin API, the session, and loading data in components
  components/           the app's frame (Layout)
    ui/                 generic pieces with no Proksi concepts (Button, Badge, Table, Field,
                        Panel, Stack, charts, icons...): the only place daisyUI's classes are used
  features/             one folder per concept (rules, flow, live, hosts, tcp...): its logic
                        and the components showing it, with their tests
  pages/                one per route, composed of features and ui components
  lib/                  helpers shared by everything (formatting)
```

Tests sit next to the file they test (`ruleModel.ts`, `ruleModel.test.ts`).

## Styles

`src/styles.css` sets Proksi's colors as two daisyUI themes, light and dark; sizes and corners are daisyUI's own. Pages are built from the components in `components/ui/` rather than class names. The build keeps only the CSS the sources use: Tailwind's classes found in `src/`, and the daisyUI parts listed in `include` there. A component that starts using another daisyUI part adds it to that list.
