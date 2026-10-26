# Changelog

## [0.17.4](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.17.3...proksi-admin-ui-v0.17.4) (2026-10-26)


### Bug Fixes

* draw latency at 0 when there are no requests, instead of a gap ([#582](https://github.com/luizfonseca/proksi/issues/582)) ([8b8bfc4](https://github.com/luizfonseca/proksi/commit/8b8bfc4917c1e9a9ce96c9a60d3b65cef5c73128))

## [0.17.3](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.17.2...proksi-admin-ui-v0.17.3) (2026-10-25)


### Bug Fixes

* use the system's allocator, which gives memory back once traffic drops ([#580](https://github.com/luizfonseca/proksi/issues/580)) ([c3ef54e](https://github.com/luizfonseca/proksi/commit/c3ef54e8f5912f998e999280bc7cc3d36574b14a))

## [0.17.2](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.17.1...proksi-admin-ui-v0.17.2) (2026-10-25)


### Miscellaneous Chores

* release 0.17.2 ([de6055b](https://github.com/luizfonseca/proksi/commit/de6055bee0e17149893cc2167d7bf9b1bbfb2399))

## [0.17.1](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.17.0...proksi-admin-ui-v0.17.1) (2026-10-24)


### Features

* keep notify blocks in the store editable in the admin UI ([#566](https://github.com/luizfonseca/proksi/issues/566)) ([8a1fbc7](https://github.com/luizfonseca/proksi/commit/8a1fbc7700c2335ad83d14e2445f7b87e4ea6039))
* show in the admin UI when a newer Proksi is released ([518be04](https://github.com/luizfonseca/proksi/commit/518be047685754cc04c7c559050f49a0a6c60bcd))


### Miscellaneous Chores

* release 0.17.1 ([880169a](https://github.com/luizfonseca/proksi/commit/880169aa88f2272fdde0ea28fffe1e63fd5e1aa0))

## [0.17.0](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.16.1...proksi-admin-ui-v0.17.0) (2026-10-22)


### Features

* add `notify` block support for webhook sending based on upstream information ([#562](https://github.com/luizfonseca/proksi/issues/562)) ([98d15a7](https://github.com/luizfonseca/proksi/commit/98d15a76492399b858bf081725dbc08e2b916ba1))
* build the admin UI with daisyUI, from shared components ([#561](https://github.com/luizfonseca/proksi/issues/561)) ([be5946e](https://github.com/luizfonseca/proksi/commit/be5946e48c5c46ea55cc5d27631bb209f6d0f6e3))
* sign in to the admin UI with a token's name and secret ([#560](https://github.com/luizfonseca/proksi/issues/560)) ([a8c6027](https://github.com/luizfonseca/proksi/commit/a8c602795b6270c8695e3ab45eda01800424ad93))

## [0.16.1](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.16.0...proksi-admin-ui-v0.16.1) (2026-10-21)


### Miscellaneous Chores

* **proksi-admin-ui:** Synchronize proksi versions

## [0.16.0](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.15.1...proksi-admin-ui-v0.16.0) (2026-10-21)


### Features

* host history, compare and rollback in the admin UI ([#536](https://github.com/luizfonseca/proksi/issues/536)) ([8e444a7](https://github.com/luizfonseca/proksi/commit/8e444a72d96222642242d93b6445a0ed9d62cdad))


### Bug Fixes

* read settings taking one value or a list as lists in the UI ([#539](https://github.com/luizfonseca/proksi/issues/539)) ([479d4e6](https://github.com/luizfonseca/proksi/commit/479d4e65e371918c1728a6d36092f6dea0072463))
* show hosts without history yet as such ([#538](https://github.com/luizfonseca/proksi/issues/538)) ([dcb3ec2](https://github.com/luizfonseca/proksi/commit/dcb3ec2a79a3f579c1a85a3ab23ff9d65c11ee64))

## [0.15.1](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.15.0...proksi-admin-ui-v0.15.1) (2026-10-20)


### Bug Fixes

* deny unwrap, expect, panic, todo and unimplemented ([#512](https://github.com/luizfonseca/proksi/issues/512)) ([7bb8066](https://github.com/luizfonseca/proksi/commit/7bb8066e4bfb95a64c3fa71b8097cb6ef800d346))

## [0.15.0](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.14.2...proksi-admin-ui-v0.15.0) (2026-10-20)


### Features

* request mirroring ([#508](https://github.com/luizfonseca/proksi/issues/508)) ([3d09f3e](https://github.com/luizfonseca/proksi/commit/3d09f3e189b30ec4f1d322d90a1e68adaad1632d))
* traffic splits ([#504](https://github.com/luizfonseca/proksi/issues/504)) ([e3e0255](https://github.com/luizfonseca/proksi/commit/e3e02557ddc142ae56ade6b104f774d26debb52c))


### Bug Fixes

* deny unwrap, expect, panic, todo and unimplemented ([#512](https://github.com/luizfonseca/proksi/issues/512)) ([7bb8066](https://github.com/luizfonseca/proksi/commit/7bb8066e4bfb95a64c3fa71b8097cb6ef800d346))

## [0.14.2](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.14.1...proksi-admin-ui-v0.14.2) (2026-10-19)


### Bug Fixes

* keep recent hosts and requests in the admin UI ([#496](https://github.com/luizfonseca/proksi/issues/496)) ([5551dac](https://github.com/luizfonseca/proksi/commit/5551dac763c995da9849b65762b7a1424083f761))
* tcp blocks in the admin UI ([#492](https://github.com/luizfonseca/proksi/issues/492)) ([e608648](https://github.com/luizfonseca/proksi/commit/e6086488e0797e72064ea0bbf3cc89a3088aeae7))

## [0.14.1](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.14.0...proksi-admin-ui-v0.14.1) (2026-10-19)


### Bug Fixes

* tcp blocks in the admin UI ([#492](https://github.com/luizfonseca/proksi/issues/492)) ([e608648](https://github.com/luizfonseca/proksi/commit/e6086488e0797e72064ea0bbf3cc89a3088aeae7))

## [0.14.0](https://github.com/luizfonseca/proksi/compare/proksi-admin-ui-v0.13.1...proksi-admin-ui-v0.14.0) (2026-10-19)


### Bug Fixes

* rule builder dropdowns ([#482](https://github.com/luizfonseca/proksi/issues/482)) ([9d97549](https://github.com/luizfonseca/proksi/commit/9d9754915574ed33e99433f7a3b6fd571c1ed74c))
