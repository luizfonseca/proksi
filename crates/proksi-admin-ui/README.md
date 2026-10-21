# proksi-admin-ui

The admin UI of [Proksi](https://github.com/luizfonseca/proksi), served by its admin listener ([ADR 0008](https://docs.proksi.info/adrs/0008-admin-ui)): a Preact application, built into `dist/` and embedded by `build.rs`.

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
    ui/                 generic pieces with no Proksi concepts: charts, icons, loading states
  features/             one folder per concept (rules, flow, live, hosts, tcp...): its logic
                        and the components showing it, with their tests
  pages/                one per route, composed of features and ui components
  lib/                  helpers shared by everything (formatting)
```

Tests sit next to the file they test (`ruleModel.ts`, `ruleModel.test.ts`).
